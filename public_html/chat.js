const VOID_UUID = "00000000-0000-0000-0000-000000000000";

class ChannelOptions {
  static ClientInvites = 1 << 0; 
  static AgentOnly       = 1 << 1; // only agents can join the channel
  static InviteOnly      = 1 << 2; // only creator or admins+ can join
  static SaveHistory     = 1 << 3; // save channel history when channel members = 0
  static Persist         = 1 << 4; // persist channel even if members = 0
  static WaitForAgent    = 1 << 5; // client can send messages but must wait for agent otherwise
  static RejoinClients   = 1 << 6; // disconnected clients are automatically rejoined on connect
  static CanNotLeave     = 1 << 7; // members may not leave the channel
  static HiddenMemberList= 1 << 8; // member list hidden from non agents
  static HiddenMessages  = 1 << 9; // messages are hidden from non agents
  static Invisible       = 1 << 10; //channel will not show up as active to non agents
  static Secret          = 1 << 11; // channel will not show up in channel listings to non agents
}

class MessageType {
  static AuthGuest(username) {
    return { "AuthGuest": username || null };
  }
  static AuthDevice(deviceuuid, username) {
    // TODO
  }
  static AuthAgent(nickname, username, password) {
    return { "AuthAgent": [nickname, username, password] };
  }
  static Join(channel) {
    return { "Join": [channel, "0.0.0.0"] };
  } 
  static Part(channel) {
    return { "Part": [VOID_UUID, channel, "0.0.0.0"] };
  }
  static Kick(channel, user, reason) {
    return { "Kick": [channel, user, reason] };
  }
  static Kill(userid, reason) {
    return { "Kill": [userid, reason] };
  }
  static Kline(ip, expires_sec, reason) {
    return { "Kline": [ip, expires_sec, reason] };
  }
  static Message(message) {
    return { "Message": message };
  }
  static Pong(epoch_time) {
    return { "Pong": epoch_time };
  }
  static Topic(title) {
    return { "Topic": title };
  }
  static Channels() {
    return { "Channels": null };
  }
  static SetChannelModes(modes) {
    return { "SetChannelModes": modes };
  }
  static Typing(channel) {
    return { "Typing": [VOID_UUID, channel] };
  } 
}
class CMessage {
  constructor(type, target, message) {
  this.id = 1;
  this.type = type;
  this.target = target; 
  this.source = VOID_UUID;
  this.message= message;
  }
}
class ChatClient {
  constructor(url) {
    this.url = url;
    this.socket = null;
    this.eventListeners = {};
    this.myid = null;
    this.myname = null;
    this.channels = {};
    this.members = [];
    this.is_agent = false;
  }

  // Connect to the WebSocket server
  connect() {
    this.socket = new WebSocket(this.url);

    // Handle connection open
    this.socket.addEventListener('open', () => {
      console.log('Connected to the chat server.');
      // authenticate
      var asGuest = document.getElementById('ichatguest').checked;
      var guestname = document.getElementById('ichatnickname').value || null;
      var username = document.getElementById('ichatusername').value || null;
      var password = document.getElementById('ichatpassword').value || null;
      
      if (asGuest) {
        var message = new CMessage(MessageType.AuthGuest(guestname), VOID_UUID, "");
        this.sendMessage(message);
      }
      else 
      {
        var message = new CMessage(MessageType.AuthAgent(guestname, username, password), VOID_UUID, "");
        this.sendMessage(message);
        this.is_agent = true;

      }
      

      this.emit('connect');
    });

    // Handle incoming messages
    this.socket.addEventListener('message', (event) => {
      console.log(event.data);
      const data = JSON.parse(event.data);
  
      switch (Object.keys(data.type)[0]) {
        case 'Typing':
          ichat_handle_typing(data.type.Typing, data.source, data.target);
          break;

        case 'ChannelModes':
          ichat_handle_chanmodes(data.type.ChannelModes, data.target);
          break;

        case 'Topic':
          ichat_handle_channel_topic(data.type.Topic, data.target);
          break;
        
        case 'Wallop':
          ichat_handle_wallop(this, data.type.Wallop);
          break;

        case 'Problem':
          ichat_show_error(null, data.type.Problem, data.message);
          break;
          
        case 'Ping':
          handle_ping(this, data);
          break;

        case 'UserList':
          ichat_handle_userlist(this, data);
          break;

        case 'ChannelList':
          ichat_handle_chanlist(this, data);
          break;

        case 'Quit':
          ichat_handle_quit(this, data);
          break;

        case 'Message':
          ichat_handle_message(this, data);
          break;

        case 'Motd':
          ichat_show_motd(data.message, data.type.Motd);
          break;

        case 'AuthOk':
          [this.myid, this.myname] = data.type.AuthOk;
          document.getElementById('ichatlogin').style.display = 'none';
          document.getElementById('ichatcontainer').style.display = 'block';
          break;
        
        case 'Part':
          ichat_handle_part(this, data);
          break;

        case 'Kick':
          ichat_handle_kick(this, data);
          break;

        // someone (maybe us) joining a channel
        case 'Join':
          let [username, ip] = data.type.Join;
          let mydate = new Date();

          if (data.source == this.myid) {
            // it is us
            this.channels[data.target] = { name: data.message, members: {} };
            this.channels[data.target].members[this.myid] = this.myname;
            this.channels[data.target].topic = "";

            // add it to the rooms tab
            let room_node = `<span id="ichat-roomtab-${data.target}" class="ichat-roomtab ichat-roomtab-active" onclick="javascript:ichat_swap_channels('${data.target}');"><i class="fa fa-list-alt"></i>(<span id="ichat-roomtab-usercount-${data.target}">0</span>) ${data.message} <a href="#" onclick="ichat_leave_channel('${data.target}', '${data.message}', chatclient);">x</a></span>`;  
            let join_status = `<div title="${mydate.toString()}" class="ichat-message-status"><i class="fa fa-arrow-right"> </i> You have joined the conversation in <strong>${data.message}</strong> as <i class="fa fa-user"> ${this.myname}</i></div>`;
            let room_container = `<div id="ichat-roomcontainer-${data.target}" oncontextmenu="ichat_rightclick_channel('${data.target}', this, event);" class="ichat-room-container">${join_status}</div>`; 
            let my_user_entry = `<div oncontextmenu="ichat_rightclick_user('${data.target}', '${data.source}', this, event);" onclick="ichat_leftclick_user(this, event);" class="ichat-user-item" id="ichat-user-entry-${data.target}-${this.myid}"><i id="ichat-user-entryicon-${data.target}-${this.myid}" title="Channel Member" class="fa fa-user"></i> ${this.myname}</div>`;
            let room_users = `<div class="ichat-user-container" id="ichat-user-container-${data.target}">${my_user_entry}</div>`;
            let room_textarea = `<textarea onkeydown="ichat_handle_input(event, this.id, this.value)" id="ichat-input-${data.target}" placeholder="Type here" class="ichat-input"></textarea>`;

            document.querySelectorAll(".ichat-roomtab-active").forEach(element => { element.classList.remove("ichat-roomtab-active"); });
            document.querySelectorAll(".ichat-room-container").forEach(element => { element.style.display = "none"; }); 
            document.querySelectorAll(".ichat-user-container").forEach(element => { element.style.display = "none"; }); 
            document.querySelectorAll(".ichat-input").forEach(element => { element.style.display = "none"; }); 
            document.getElementById('ichat-room-list').innerHTML += room_node;
            let chanmen = generate_channel_menu(data.target, data.message);
            // alert(chanmen);
            document.getElementById('ichat-message-list').innerHTML += room_container + chanmen; 
            document.getElementById('ichat-user-list').innerHTML += room_users + generate_user_menu(data.source, data.target, this.myname, this.is_agent);
            document.getElementById('ichat-text-container').innerHTML += room_textarea;

            // console.log(this.channels);
          } else {
            if (!this.channels[data.target]) {
              console.log("We got a JOIN for channel we have no record of: " + data.target);
              break;
            }

            let chan_container = document.getElementById("ichat-roomcontainer-" + data.target);
            let user_container = document.getElementById("ichat-user-container-" + data.target);
            let channel_tab = document.getElementById('ichat-roomtab-' + data.target);
            let usercount_container = document.getElementById('ichat-roomtab-usercount-' + data.target);
            usercount_container.innerText = parseInt(usercount_container.innerText) + 1;

            if ((!channel_tab.classList.contains('ichat-roomtab-active')) && (!channel_tab.classList.contains('ichat-roomtab-newmessage'))) {
              channel_tab.classList.add('ichat-roomtab-newmessage');
            }
            if (!chan_container || !user_container) {
              console.log("Missing channel or user container for " + data.target);
              break;
            }
           
            let ipstring = '';

            if (ip != '0.0.0.0') {
              ipstring = '@' + ip;
            }


            let join_status = `<div title="${mydate.toString()}" class="ichat-message-status"><i class="fa fa-arrow-right"> </i> ${username}${ipstring} has joined the conversation.</div>`;
            let my_user_entry = `<div onclick="ichat_leftclick_user(this, event);" oncontextmenu="ichat_rightclick_user('${data.target}', '${data.source}', this, event);" class="ichat-user-item" id="ichat-user-entry-${data.target}-${data.source}"><i id="ichat-user-entryicon-${data.target}-${data.source}" title="Channel Member" class="fa fa-user"></i> ${username}</div>`;
            chan_container.innerHTML += join_status;
            user_container.innerHTML += my_user_entry + generate_user_menu(data.source, data.target, username, this.is_agent, this.is_agent);
            chan_container.scrollTo({
              top: chan_container.scrollHeight,
              behavior: 'smooth'
            });
            
        }
      
        break;
      }

      this.emit('message', data);
    });

    // Handle connection close
    this.socket.addEventListener('close', () => {
      console.log('Disconnected from the chat server.');
      document.getElementById('ichatloginbutton').disabled = false;
      ichat_show_error(null, "Connection Closed", "Your connection to the chat server was closed by the host.");
      // alert("Your chat session has ended");
      document.getElementById('ichatcontainer').style.display = 'none';
      document.getElementById('ichatlogin').style.display = 'block';
      document.querySelectorAll('.ichat-roomtab').forEach(element => element.remove());
      document.querySelectorAll('.ichat-room-container').forEach(element => element.remove());
      document.querySelectorAll('.ichat-user-container').forEach(element => element.remove());
      document.querySelectorAll('.ichat-input').forEach(element => element.remove());
      this.emit('disconnect');
    });

    // Handle errors
    this.socket.addEventListener('error', (error) => {
      console.error('WebSocket error:', error);
      this.emit('error', error);
    });
  }

  // Send a message to the server
  sendMessage(message) {
    if (this.socket && this.socket.readyState === WebSocket.OPEN) {
      const data = JSON.stringify(message);
      // const data = JSON.stringify({ type: 'Message', message });
      this.socket.send(data);
    } else {
      console.warn('Cannot send message: WebSocket is not open.');
    }
  }

  // Add an event listener
  on(event, callback) {
    if (!this.eventListeners[event]) {
      this.eventListeners[event] = [];
    }
    this.eventListeners[event].push(callback);
  }

  // Emit an event to all listeners
  emit(event, data) {
    const listeners = this.eventListeners[event];
    if (listeners) {
      listeners.forEach(callback => callback(data));
    }
  }

  // Disconnect from the WebSocket server
  disconnect() {
    if (this.socket) {
      this.socket.close();
      document.getElementById('ichatloginbutton').disabled = false;
    }
  }
}

function ichat_handle_wallop(mychatclient, data) {
  let mydate = new Date();
  message_div = `<div title="${mydate.toString()}" class="ichat-message-status ichat-message-status-wallop"><i class="fa fa-bullhorn"> </i> operators: ${data} </div>`;

  document.querySelectorAll('.ichat-room-container').forEach(element => element.innerHTML += message_div);
}

function ichat_handle_kick(mychatclient, data) {
  [channel, target, reason] = data.type.Kick;

  let chan_container = document.getElementById('ichat-roomcontainer-' + data.target);

  if (target == mychatclient.myid) {
    let channel_name = mychatclient.channels[channel].name || 'unknown channel';
    alert("You have been kicked from '" + channel_name + "'\n\nReason: " + reason);
    let tab = document.getElementById('ichat-roomtab-' + data.target);
    let users = document.getElementById('ichat-user-container-' + data.target);
    let editbox = document.getElementById('ichat-input-' + data.target);
    let chan_menu = document.getElementById('ichat-channel-menu-' + data.target);

    editbox.remove();
    users.remove();
    tab.remove();
    chan_menu.remove();
    chan_container.remove();

    if (document.querySelector('.ichat-roomtab') != null)
    {
      document.querySelector('.ichat-roomtab').click();
    }
    return;
  }

  let user_count = document.getElementById('ichat-roomtab-usercount-' + channel);
  let user_entry = document.getElementById('ichat-user-entry-' + channel + '-' + target);
  let mydate = new Date();
  let part_message = `<div title="${mydate.toString()}" class="ichat-message-status ichat-message-status-red"><i class="fa fa-arrow-left"> </i> ${data.message} was kicked from the channel (${reason})</div>`;
  chan_container.innerHTML += part_message;
  chan_container.scrollTo({
    top: chan_container.scrollHeight,
    behavior: 'smooth'
  });
  
  if (user_entry == null) {
    console.log(`failed to remove user entry quit on ${data.target}-${data.source} : ${data.message}`);
  }
  user_entry.remove();
  user_count.innerText -= 1;


}

function ichat_handle_chanlist(mychatclient, data) {
  ichat_show_channel_list();
  let channel_list_container = document.getElementById('ichat-channel-list-rows');
  channel_list_container.innerHTML = 'Processing....';
  let newrows = '';
  let channels = data.type.ChannelList;
  channels.forEach((channel) => newrows += `<tr><td><a href="#" onclick="javascript:chatclient.sendMessage(new CMessage(MessageType.Join('${channel[1]}'), '${VOID_UUID}', ''));"> ${channel[1]}</a></td><td>${channel[2]}</td><td>${channel[3]}</td></tr>`);
  channel_list_container.innerHTML = newrows;

}
function ichat_handle_part(mychatclient, data) {
  [source, target, ip] = data.type.Part;

  if (source == mychatclient.myid) {
    let tab = document.getElementById('ichat-roomtab-' + data.target);
    let room = document.getElementById('ichat-roomcontainer-' + data.target);
    let users = document.getElementById('ichat-user-container-' + data.target);
    let editbox = document.getElementById('ichat-input-' + data.target);

    editbox.remove();
    users.remove();
    room.remove();
    tab.remove();

    if (document.querySelector('.ichat-roomtab') != null)
    {
      document.querySelector('.ichat-roomtab').click();
    }
    return;
  }

  // not me
  let container = document.getElementById('ichat-roomcontainer-' + data.target);
  let user_count = document.getElementById('ichat-roomtab-usercount-' + data.target);
  let user_entry = document.getElementById('ichat-user-entry-' + data.target + '-' + data.source);
  let mydate = new Date();
  let part_message = `<div title="${mydate.toString()}" class="ichat-message-status ichat-message-status-red"><i class="fa fa-arrow-left"> </i> ${data.message} has left the channel</div>`;
  container.innerHTML += part_message;
  container.scrollTo({
    top: container.scrollHeight,
    behavior: 'smooth'
  });
  
  if (user_entry == null) {
    console.log(`failed to remove user entry quit on ${data.target}-${data.source} : ${data.message}`);
  }
  user_entry.remove();
  user_count.innerText -= 1;

}
function ichat_leave_channel(channelid, channelname, mychatclient) {
  if (confirm("Are you sure you want to leave the channel?\n\n" + channelname))
  {
    mychatclient.sendMessage(new CMessage(MessageType.Part(channelid), channelid, ""));
  }
}

function ichat_handle_typing(data, source, target) {
  let div = document.getElementById('ichat-user-entryicon-' + target + '-' + source);

  if (div) {
    div.classList.remove('fa-user');
    div.classList.add('fa-keyboard-o');
    div.classList.add('ichat-slowfadeio');
    div.title = "Member is typing...";

    setTimeout(() => {
      div.classList.remove('fa-keyboard-o');
      div.classList.remove('ichat-slowfadeio');
      div.classList.add('fa-user');
      div.title = "Channel Member";
    }, 4000);
  }

}
function ichat_handle_channel_topic(topic, target) {
  let div = document.getElementById('ichat-roomcontainer-' + target);
  div.innerHTML += `<div class="ichat-message-status"><i class="fa fa-book"> </i> Topic: ${topic}</div>`;
  chatclient.channels[target].topic = topic;
}

function ichat_handle_chanmodes(modes, target) {
  setTimeout(() => { // hack because the div is not ready right away on new joins
    let mydate = new Date();
    let div = document.getElementById('ichat-roomcontainer-' + target);
    let content = '';
    chatclient.channels[target].modes = modes;

    modes.forEach((mode) => content += '<li>' + mode + '</li>');
    let message = `<div title="${mydate.toString()}" class="ichat-message-status ichat-message-status-modes"><i class="fa fa-gear"> </i> Channel has the following settings:<br /><ul class="ichat-modes-list">${content}</ul></div>`;
    div.innerHTML += message;
 }, 500);
}

function ichat_is_typing_sender(channelid, mychatclient) {
  if (!type_map.has(channelid)) {
    type_map.set(channelid, 1);
    mychatclient.sendMessage(new CMessage(MessageType.Typing(channelid), channelid, ""));
    setTimeout(() => {
      type_map.delete(channelid);
    }, 5000);
  }
}
function ichat_parse_command_string(channelid, command, args, mychatclient) {
  // alert("commanding " + command + " and " + args);
  switch (command.toUpperCase()) {
    case 'ChannelModes':

      break;
    case 'JOIN':
      if (args == null) {
        ichat_show_error(null, "Missing Channel Argument", "The JOIN command requires a channel parameter, either a uuid, or room name.");
        return -1
      }

      args = args.join(" ");

      if (args.match(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i)) {
        return new CMessage(MessageType.Join(""), args, "");
      }

      if (args.match(/^[a-zA-Z0-9\- ]{3,50}$/))
      {
        return new CMessage(MessageType.Join(args), VOID_UUID, "");
      }

      ichat_show_error(null, "Invalid Channel Argument", "The JOIN command requires a channel parameter, either a uuid, or room name (a-zA-z0-9 {3,50})");
      return -1;

    case 'PART':
      if (args == null) {
        ichat_show_error(null, "Missing Channel Argument", "The PART command requires a channel uuid parameter.");
        return -1
      }

      args = args.join(" ");
      console.log(args);
      if (args.match(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i)) {
        return new CMessage(MessageType.Part(args), args, "");
      }

      ichat_show_error(null, "Invalid Channel Argument", "The PART command requires a channel uuid parameter");
      return -1;

    case 'CHANNELS':
      return new CMessage(MessageType.Channels(), VOID_UUID, "");

    case 'WHOIS':

      break;

    case 'TITLE':

      break;

    case 'KICK':

      break;

    case 'KILL':
      
      break;

    case 'KLINE':
      let ip = args.shift();
      let expires_sec = args.shift();

      if (!Number.isInteger(expires_sec))
        expires_sec = 3600;

      let reason = args.join(" ");
      if (reason == "") {
        reason = "none given";
      }

      if (!ip.match(/^[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}$/)) {
        ichat_show_error(null, "Invalid KLine Ip", "The paramaters are <ip> <expiry_in_seconds> <reason>");
        return -1;
      }

      return new CMessage(MessageType.Kline(ip, expires_sec, reason), VOID_UUID, "");

    case 'MODIFY':

      break;

    case 'STATS':

      break;

    case 'TYPING':

      break;
  }

  return null;
}

function ichat_show_motd(title, message) {
  let content = `
    <div class="ichat-error">
    <div class="ichat-error-header"><i class="fa fa-book"> </i> ${title}</div> 
<div class="ichat-error-message">${message}</div>
      <div onclick="this.parentNode.remove(); "class="ichat-error-dismiss"><i class="fa fa-smile-o"></i> Dismiss</div>
    </div>
`;
  document.body.innerHTML += content;
}

function ichat_show_error(parentid, title, message) {
  let element = document.getElementById(parentid);

  if (!element)
    element = document.body;

  let content = `
    <div class="ichat-error" >
      <div class="ichat-error-header"><i class="fa fa-gear"> </i> Error: ${title}</div>
      <div class="ichat-error-message">${message}</div>
      <div onclick="this.parentNode.remove(); "class="ichat-error-dismiss"><i class="fa fa-frown-o"></i> Dismiss</div>
    </div>
    `;

  element.innerHTML += content;
}

function generate_channel_menu(id, channel) {
  return `<div onmouseout="if (!this.contains(event.relatedTarget)) { this.style.display = 'none'; }" id="ichat-channel-menu-${id}" class="ichat-rightclickmenu" style="display: none" onclick="javascript:this.style.display = 'none';">
    <div class="ichat-rightclickmenu-header"><i class="fa fa-list-alt"> </i> ${channel}</div>
    <div onclick="ichat_get_channel_info(chatclient, '${id}');" class="ichat-rightclickmenu-option"><i class="fa fa-info-circle"> </i> Channel Information</div>
    </div>
`;
}
function ichat_save_channel_settings(mychatclient) {
  let chanid_element = document.getElementById('ichat-settings-channel-id');

  if (chanid_element == null) {
    alert("Channel id is not set for this function.");
    return;
  }

  let chanid = chanid_element.value;
  let topic = document.getElementById('ichat-settings-channel-topic').value;

  if (mychatclient.channels[chanid].topic != topic) { 
    mychatclient.sendMessage(new CMessage(MessageType.Topic(topic), chanid, ""));
  }

  let modes = 0;
  
  if (document.getElementById('ichat-setting-chan-clientinvites').checked)
    modes |= ChannelOptions.ClientInvites;

  if (document.getElementById('ichat-setting-chan-agentonly').checked)
    modes |= ChannelOptions.AgentOnly;

  if (document.getElementById('ichat-setting-chan-inviteonly').checked)
    modes |= ChannelOptions.InviteOnly;

  if (document.getElementById('ichat-setting-chan-savehistory').checked)
    modes |= ChannelOptions.SaveHistory;

  if (document.getElementById('ichat-setting-chan-persist').checked)
    modes |= ChannelOptions.Persist;

  if (document.getElementById('ichat-setting-chan-rejoinclients').checked)
    modes |= ChannelOptions.RejoinClients;

  if (document.getElementById('ichat-setting-chan-waitforagent').checked)
    modes |= ChannelOptions.WaitForAgent;

  if (document.getElementById('ichat-setting-chan-cannotleave').checked)
    modes |= ChannelOptions.CanNotLeave;

  if (document.getElementById('ichat-setting-chan-hiddenmemberlist').checked)
    modes |= ChannelOptions.HiddenMemberList;

  if (document.getElementById('ichat-setting-chan-hiddenmessages').checked)
    modes |= ChannelOptions.HiddenMessages;

  if (document.getElementById('ichat-setting-chan-invisible').checked)
    modes |= ChannelOptions.Invisible;

  if (document.getElementById('ichat-setting-chan-secret').checked)
    modes |= ChannelOptions.Secret;

  if (modes != mychatclient.channels[chanid].modes) {
    mychatclient.sendMessage(new CMessage(MessageType.SetChannelModes(modes), chanid, ""));
  }

  document.getElementById('ichat-channel-settings-dialog').style.display = 'none';
}

function ichat_get_channel_info(mychatclient, id) {
  document.getElementById('ichat-settings-channel-name').value = mychatclient.channels[id].name;
  document.getElementById('ichat-settings-channel-topic').value = mychatclient.channels[id].topic || "";
  document.getElementById('ichat-settings-channel-id').value = id;

  console.log("modes:" + mychatclient.channels[id].modes);
  if (mychatclient.channels[id].modes.includes('Allow Invites')) {
    document.getElementById('ichat-setting-chan-clientinvites').checked = true;
  } else {
    document.getElementById('ichat-setting-chan-clientinvites').checked = false;
  }

  if (mychatclient.channels[id].modes.includes('Agent Only')) {
    console.log("agent only");
    document.getElementById('ichat-setting-chan-agentonly').checked = true;
  }else {
    document.getElementById('ichat-setting-chan-agentonly').checked = false;
  }

  if (mychatclient.channels[id].modes.includes('Invite Only')) {
    document.getElementById('ichat-setting-chan-inviteonly').checked = true;
  }else {
    document.getElementById('ichat-setting-chan-inviteonly').checked = false;
  }

  if (mychatclient.channels[id].modes.includes('Save History')) {
    document.getElementById('ichat-setting-chan-savehistory').checked = true;
  }else {
    document.getElementById('ichat-setting-chan-savehistory').checked = false;
  }

  if (mychatclient.channels[id].modes.includes('Persist Empty')) {
    document.getElementById('ichat-setting-chan-persist').checked = true;
  }else {
    document.getElementById('ichat-setting-chan-persist').checked = false;
  }

  if (mychatclient.channels[id].modes.includes('Rejoin on Disconnect')) {
    document.getElementById('ichat-setting-chan-rejoinclients').checked = true;
  }else {
    document.getElementById('ichat-setting-chan-rejoinclients').checked = false;
  }

  if (mychatclient.channels[id].modes.includes('Wait for Agent')) {
    document.getElementById('ichat-setting-chan-waitforagent').checked = true;
  }else {
    document.getElementById('ichat-setting-chan-waitforagent').checked = false;
  }

  if (mychatclient.channels[id].modes.includes('May Not Leave')) {
    document.getElementById('ichat-setting-chan-cannotleave').checked = true;
  }else {
    document.getElementById('ichat-setting-chan-cannotleave').checked = false;
  }

  if (mychatclient.channels[id].modes.includes('Hidden Members')) {
    document.getElementById('ichat-setting-chan-hiddenmemberlist').checked = true;
  }else {
    document.getElementById('ichat-setting-chan-hiddenmemberlist').checked = false;
  }

  if (mychatclient.channels[id].modes.includes('Hidden Messages')) {
    document.getElementById('ichat-setting-chan-hiddenmessages').checked = true;
  }else {
    document.getElementById('ichat-setting-chan-hiddenmessages').checked = false;
  }

  if (mychatclient.channels[id].modes.includes('Invisible')) {
    document.getElementById('ichat-setting-chan-invisible').checked = true;
  }else {
    document.getElementById('ichat-setting-chan-invisible').checked = false;
  }

  if (mychatclient.channels[id].modes.includes('Not Listed')) {
    document.getElementById('ichat-setting-chan-secret').checked = true;
  }else {
    document.getElementById('ichat-setting-chan-secret').checked = false;
  }

  document.getElementById('ichat-channel-settings-dialog').style.display = 'block';
}

function ichat_rightclick_channel(channel, obj, event) {
  event.preventDefault();
  // ichat_leftclick_user(obj, event);
  let submenu = document.getElementById('ichat-channel-menu-' + channel);
  submenu.style.top = event.clientY - 10 + 'px';
  submenu.style.left = event.clientX - 50 + 'px';
  submenu.style.display = 'block';
  
}

function ichat_kline_user(userid) {
  let expiry_sec = prompt("Please choose an expiry in seconds, default is 1 hour", 3600);
  let reason = prompt("Reason for the server ban:", "No reason provided");
  // chatclient.

}
function ichat_kill_user(userid) {
  let reason = prompt("Please choose a kill reason:\n", "no reason was provided");
  chatclient.sendMessage(new CMessage(MessageType.Kill(userid, reason), VOID_UUID, ""));
}

function ichat_kick_channel(channel, userid) {
  let reason = prompt("Please choose a kick reason:\n", "no reason was provided");
  chatclient.sendMessage(new CMessage(MessageType.Kick(channel, userid, reason), channel, ""));
}

function generate_user_menu(id, channel, username, is_admin) {
  if (is_admin) {
    return `<div onmouseout="if (!this.contains(event.relatedTarget)) { this.style.display = 'none'; }" id="ichat-user-menu-${channel}-${id}" class="ichat-rightclickmenu" style="display: none" onclick="javascript: this.style.display = 'none';">
    <div class="ichat-rightclickmenu-header"><i class="fa fa-info-circle"></i> ${username}</div>
    <div onclick="ichat_kick_channel('${channel}', '${id}', '${username}');" class="ichat-rightclickmenu-option"><i class="fa fa-eject"> </i> Kick from Channel</div>
<hr>
    <div onclick="ichat_kill_user('${id}', '${username}');" class="ichat-rightclickmenu-option"><i class="fa fa-eject"> </i> Kick from Server</div>
<div onclick="ichat_kline_user('${id}', '${username}');" class="ichat-rightclickmenu-option"><i class="fa fa-ban"> </i> Ban from Server</div>

<hr/>
    <div onclick="ichat_whois('${id}');" class="ichat-rightclickmenu-option"><i class="fa fa-question-circle"> </i> Whois</div>
    </div>
  `;
  }
  else
  {
    return `<div onmouseout="if (!this.contains(event.relatedTarget)) { this.style.display = 'none'; }" id="ichat-user-menu-${channel}-${id}" class="ichat-rightclickmenu" style="display: none" onclick="javascript: this.style.display = 'none';">
    <div class="ichat-rightclickmenu-header"><i class="fa fa-info-circle"></i> ${username}</div>
    <div onclick="ichat_whois('${id}');" class="ichat-rightclickmenu-option"><i class="fa fa-question-circle"> </i> Whois</div>
    </div>`;
  }
}
function ichat_leftclick_user(obj, event) {
  document.querySelectorAll('.ichat-user-item').forEach(element => element.classList.remove('ichat-user-item-selected'));
  obj.classList.add('ichat-user-item-selected');
}

function ichat_rightclick_user(channel, userid, obj, event) {
  event.preventDefault();
  ichat_leftclick_user(obj, event);
  let submenu = document.getElementById('ichat-user-menu-' + channel + '-' + userid);
  submenu.style.top = event.clientY - 10 + 'px';
  submenu.style.left = event.clientX - 50 + 'px';
  submenu.style.display = 'block';
  
}

function ichat_handle_userlist(obj, data) {
  let users = data.type.UserList;
  let user_container = document.getElementById('ichat-user-container-' + data.target);
  let user_count = document.getElementById('ichat-roomtab-usercount-' + data.target);

  users.forEach(grouping => { 
    if (grouping[0] != obj.myid) { 
      if (!document.getElementById('ichat-user-entry-' + data.target + '-' + grouping[0])) {
        let new_user = `<div oncontextmenu="ichat_rightclick_user('${data.target}', '${grouping[0]}', this, event);" onclick="ichat_leftclick_user(this, event);" id="ichat-user-entry-${data.target}-${grouping[0]}" class="ichat-user-item"><i id="ichat-user-entryicon-${data.target}-${grouping[0]}" title="Channel Member" class="fa fa-user"></i> ${grouping[1]}</div>`;
        // alert(data.target);
        user_container.innerHTML += new_user;
        user_container.innerHTML += generate_user_menu(grouping[0], data.target, grouping[1], obj.is_agent);
      }
    }
  });
  user_count.innerText = parseInt(user_count.innerText) + users.length;
}

function ichat_handle_quit(obj, data) {
  let reason = data.type.Quit;
  let container = document.getElementById('ichat-roomcontainer-' + data.target);
  let usercount = document.getElementById('ichat-roomtab-usercount-' + data.target);
  let user_entry = document.getElementById('ichat-user-entry-' + data.target + '-' + data.source);
  let mydate = new Date();
  let quit_message = `<div title="${mydate.toString()}" class="ichat-message-status ichat-message-status-red"><i class="fa fa-times-circle"> </i> ${data.message} has disconnected from the server [${reason}]</div>`;
 
  container.innerHTML += quit_message;
  container.scrollTo({
    top: container.scrollHeight,
    behavior: 'smooth'
  });
  
  if (user_entry == null) {
    console.log(`failed to remove user entry quit on ${data.target}-${data.source} : ${data.message}`);
  }
  user_entry.remove();
  usercount.innerText -= 1;
  
}

function ichat_handle_message(obj, data) {
  let message = data.type.Message;
  if (data.source == obj.myid) {
    return; // skip our own as we already print it.
  } 
  let channel_tab = document.getElementById('ichat-roomtab-' + data.target);

  if ((!channel_tab.classList.contains('ichat-roomtab-newmessage')) && (!channel_tab.classList.contains('ichat-roomtab-active'))) {
    channel_tab.classList.add('ichat-roomtab-newmessage');
  }

  let container = document.getElementById('ichat-roomcontainer-' + data.target);
  let mydate = new Date();
  let theymessage = `<div class="ichat-message-other" title="sent: ${mydate.toString()}"><span class="ichat-namedisplay">${data.message}:</span> <span class="ichat-messagedisplay">${message}</span></div>`;
  container.innerHTML += theymessage;
  // container.scrollTop = container.scrollHeight;
  container.scrollTo({
    top: container.scrollHeight,
    behavior: 'smooth'
  });
}

function ichat_swap_channels(id) {
  let channel_tab = document.getElementById('ichat-roomtab-' + id);
  let channel_messages = document.getElementById('ichat-roomcontainer-' + id);
  let channel_users = document.getElementById('ichat-user-container-' + id);
  let channel_input = document.getElementById('ichat-input-' + id);

  if (!channel_tab || !channel_messages || !channel_users) {
    alert("channel swap failed; unable to locate a required div section.");
    return;
  }

  document.querySelectorAll(".ichat-input").forEach(element => { element.style.display = "none"; }); 
  document.querySelectorAll('.ichat-roomtab-active').forEach(element => element.classList.remove('ichat-roomtab-active'));
  document.querySelectorAll('.ichat-room-container').forEach(element => element.style.display = 'none');
  document.querySelectorAll('.ichat-user-container').forEach(element => element.style.display = 'none');
  channel_tab.classList.add('ichat-roomtab-active');
  channel_tab.classList.remove('ichat-roomtab-newmessage');
  channel_messages.style.display = 'block';
  channel_users.style.display = 'block';
  channel_input.style.display = 'block';
}

function ichat_show_channel_list() {
  let channel_tab = document.getElementById('ichat-roomtab-roomlist');
  let channel_list_container = document.getElementById('ichat-channel-list-container');
  document.querySelectorAll(".ichat-input").forEach(element => { element.style.display = "none"; }); 
  document.querySelectorAll('.ichat-roomtab-active').forEach(element => element.classList.remove('ichat-roomtab-active'));
  document.querySelectorAll('.ichat-room-container').forEach(element => element.style.display = 'none');
  document.querySelectorAll('.ichat-user-container').forEach(element => element.style.display = 'none');
  channel_tab.classList.add('ichat-roomtab-active');
  channel_tab.classList.remove('ichat-roomtab-newmessage');
  channel_list_container.style.display = 'block';

}
/////////////////////////////////////////////
function handle_ping(ob, data) {
  ping = data.type.Ping;
  ob.sendMessage(new CMessage(MessageType.Pong(ping), VOID_UUID, ""));

}

