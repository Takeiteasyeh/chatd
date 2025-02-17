use std::{str::FromStr, time::SystemTime};
use crate::{handle_client_error, AuthFinder, ClientStatus, Config, OpenOptions, Write};
// use regex::Regex;
use futures_util::SinkExt;
use crate::{
    IpAddr, Ipv4Addr, message::*,
    channel::*, client::Client, server::Server, Arc, CMessage, ClientOptions, ClientType, Mutex, ProblemCode, RwLock, Uuid
};

pub struct CommandHandler;
impl CommandHandler {
    pub async fn kline(server: &Mutex<Server>, client: &RwLock<Client>, target: IpAddr, reason: String, expires_sec: u64) {
        if client.read().await.gettype() as u8 != ClientType::Agent as u8 {
            _ = client.write().await.sender().lock().await.send(CMessage::new_problem(ProblemCode::PermissionDenied, None, "No permission to perform this action.".to_string()).into()).await;
            return;
        }

        if server.lock().await.ban_exists(target).await {
            _ = client.write().await.sender().lock().await.send(CMessage::new_problem(ProblemCode::AlreadyMember, None, "The ip is already in the ban list.".to_string()).into()).await;
            return;
        }

        let mut slock = server.lock().await;
        let expires = SystemTime::now().checked_add(std::time::Duration::new(expires_sec,0)).unwrap();
        match slock.add_ban(target, &reason, expires, &client.read().await.name()).await {
            Ok(()) => {},
            Err(_e) => {
                _ = client.write().await.sender().lock().await.send(CMessage::new_problem(ProblemCode::NotAvailable, None, "Unable to ban ip.".to_string()).into()).await;
                return;
            }
        }

        // kill all clients that match it, and arent admins
        let hunt_clients = slock.get_clients().await.unwrap();
        let hunt_lock = hunt_clients.lock().await;
        let mut found_clients: Vec<Uuid> = Vec::new();

        for (id, check_client) in hunt_lock.iter() {
            let clock = check_client.write().await;
            if clock.ip() != target {
                continue;
            }

            if clock.gettype() as u8 == ClientType::Agent as u8 {
                println!("skip kline matching kill of agent: {}@{} [{}]", clock.name(), clock.ip(), reason.to_owned());
                continue;
            }

            // matches, get rid of em after the loop locks
            found_clients.push(clock.id());
        }

        std::mem::drop(hunt_lock);
        let reason = format!("Banned ({})", reason);
        let kicker_name = client.read().await.name();
        let ban_file = slock.conf.ban_db.to_owned();
        let bans = slock.bans();
        slock.sendto_wallops(CMessage::new(MessageType::Wallop(
            format!("<i>{}</i> server banned <i>{}</i> affecting <strong>{}<strong> clients ({})", kicker_name, target.to_string(), found_clients.len(), reason.clone())), Uuid::nil(), Uuid::nil(), None)).await;

        std::mem::drop(slock);

        for found in found_clients {
            CommandHandler::kill(server, client, found, reason.clone()).await;
        }

        _ = crate::Ban::save_to_disk(&ban_file, &bans.read().await.clone());
    }

    pub async fn kill(server: &Mutex<Server>, client: &RwLock<Client>, target: Uuid, mut reason: String) {
        if client.read().await.gettype() as u8 != ClientType::Agent as u8 {
            _ = client.write().await.sender().lock().await.send(CMessage::new_problem(ProblemCode::PermissionDenied, None, "No permission to perform this action.".to_string()).into()).await;
            return;
        }

        let target_ref = server.lock().await.get_client_ref(target).await;

        if target_ref.is_none() {
            _ = client.write().await.sender().lock().await.send(CMessage::new_problem(ProblemCode::NameInvalid, None, "No matching client was found for KILL.".to_string()).into()).await;
            return;
        }

        let target_ref = target_ref.unwrap();

        if target_ref.read().await.has_options(ClientOptions::Admin) {
            println!("options: {}", target_ref.read().await.options().bits());
            _ = client.write().await.sender().lock().await.send(CMessage::new_problem(ProblemCode::PermissionDenied, None, "You may not kill ADMIN flag users.".to_string()).into()).await;
            return;
        }

        reason = reason.replace('<', "&lt;");
        reason = reason.replace('>', "&gt;");
        let length = reason.char_indices().count();

        if length > 255 {
            _ = client.write().await.sender().lock().await.send(CMessage::new_problem(ProblemCode::InvalidArgument, None, "Reason must be under 255 characters.".to_string()).into()).await;
            return;
        }

        if length == 0 {
            reason = "no reason was provided.".to_string();
        }

        let target_channels = target_ref.read().await.channels().await;
        let kicker_name = client.read().await.name();
        let kicked_name = target_ref.read().await.name();
        let kicked_ip = target_ref.read().await.ip();

        for (id, _name) in target_channels {
            let chan_ref = server.lock().await.get_channel_ref(id).await;

            if let Some(chan_ref) = chan_ref {
                _ = chan_ref.write().await.sendto_all_butone(target, &CMessage::new(MessageType::Quit(format!("Killed ({})", reason.to_owned())), target, id, Some(kicked_name.clone()))).await;
                chan_ref.write().await.remove_member(target).await;
                let is_persist = chan_ref.read().await.has_option(ChannelOptions::Persist);

                if chan_ref.read().await.count_members().await == 0 && !is_persist {
                    let mut slock = server.lock().await;

                    if chan_ref.read().await.has_option(ChannelOptions::SaveHistory) {
                        let logfile = OpenOptions::new()
                            .append(true)
                            .create(true)
                            .open(format!("logs/{}.log",id));

                        if logfile.is_ok() {
                            println!("logfile is ok");
                            let log_entry = format!("{:?} / DESTROY: {} ({}@{}) [Client Killed]\n", std::time::SystemTime::now(), chan_ref.read().await.name(), kicked_name , target_ref.read().await.ip());

                            let _ = logfile.unwrap().write(log_entry.as_bytes());
                        }
                    }
                    slock.remove_channel(id).await;
                } else {
                    if chan_ref.read().await.has_option(ChannelOptions::SaveHistory) {
                        let logfile = OpenOptions::new()
                            .append(true)
                            .create(true)
                            .open(format!("logs/{}.log", id));

                        if logfile.is_ok() {
                            println!("logfile is ok");
                            let log_entry = format!("{:?} / Quit(Killed): {} ({}@{})\n", std::time::SystemTime::now(), chan_ref.read().await.name(), kicked_name , target_ref.read().await.ip());

                            let _ = logfile.unwrap().write(log_entry.as_bytes());
                        }
                    }
                }        
            }
        }
        target_ref.write().await.clear_channel_list().await;
        _ = target_ref.write().await.sender().lock().await.send(CMessage::new_problem(ProblemCode::KickedFromServer, None, format!("You were kicked from the server: {}", reason).to_string()).into()).await;
        _ = target_ref.write().await.sender().lock().await.close().await;
        server.lock().await.remove_client(target).await;
        server.lock().await.sendto_wallops(CMessage::new(MessageType::Wallop(format!("<i>{}</i> killed <i>{}@{}</i> ({})", kicker_name, kicked_name, kicked_ip, reason)), Uuid::nil(), Uuid::nil(), None)).await;
    }

    pub async fn kick(server: &Mutex<Server>, client: &RwLock<Client>, channel: Uuid, user: Uuid, reason: String) {
        let chan_ref = server.lock().await.get_channel_ref(channel).await; 
        if let Some(chan_ref) = chan_ref {
            if chan_ref.read().await.owner_id().unwrap_or(Uuid::nil()) != client.read().await.id() && client.read().await.gettype() as u8 != ClientType::Agent as u8 {
                _ = client.write().await.sender().lock().await.send(CMessage::new_problem(ProblemCode::PermissionDenied, Some(channel), "Cannot kick from channels that do not belong to you.".to_string()).into()).await;
                return;
            }

            if !chan_ref.read().await.is_member(user).await {
                _ = client.write().await.sender().lock().await.send(CMessage::new_problem(ProblemCode::NotMember, Some(channel), "User is not a member of that channel.".to_string()).into()).await;
                return;
            }

            let target_client = server.lock().await.get_client_ref(user).await;

            if target_client.is_none() {
                _ = client.write().await.sender().lock().await.send(CMessage::new_problem(ProblemCode::NotMember, Some(channel), "User is not a member of that channel.".to_string()).into()).await;
                return;
            }

            let target_name = target_client.clone().unwrap().read().await.name();
            _ = chan_ref.write().await.sendto_all(&CMessage::new(MessageType::Kick(channel, user, reason), Uuid::nil(), channel, Some(target_name)).into()).await;
            chan_ref.write().await.remove_member(user).await;
            target_client.unwrap().write().await.remove_channel(channel).await;
            
            return;
        } else {
            _ = client.write().await.sender().lock().await.send(CMessage::new_problem(ProblemCode::ChannelInvalid, Some(channel), "Cannot kick from non-existent channels.".to_string()).into()).await;
            return;
        }

    }

    pub async fn set_channel_modes(server: &Mutex<Server>, client: &RwLock<Client>, channel: Uuid, modes: u64) {
        let chan_ref = server.lock().await.get_channel_ref(channel).await;

        if chan_ref.is_none() {
            let _ = client.write().await.sender().lock().await.send(CMessage::new_problem(ProblemCode::ChannelInvalid, Some(channel), "Cannot change modes of non-existent channels.".to_string()).into()).await;
            return;
        }

        let chan_ref = chan_ref.unwrap();
        let mut client_lock = client.write().await;
        let chan_lock = chan_ref.read().await;
        let chan_owner = chan_lock.owner_id().unwrap_or(Uuid::nil());

        if chan_owner != client_lock.id() && client_lock.gettype() as u8 != ClientType::Agent as u8 {
            let _ = client_lock.sender().lock().await.send(CMessage::new_problem(ProblemCode::PermissionDenied, Some(channel), "You have no permission to change this channels modes.".to_string()).into()).await;
            return;
        }

        let mut channel_options = 0u64;

        let real_options: Vec<u64> = ChannelOptions::available().iter().filter_map(|&option| { 
            if modes & (option as u64) == (option as u64) { 
                channel_options += option as u64;
                Some(option as u64); 
            } 
            None
        }).collect();

        // if !real_options.is_empty() {
        //     for option in real_options {
        //         println!("ok with option: {}", option as u64);
        //         channel_options += option;
        //     }
        // }

        std::mem::drop(chan_lock);
        let mut chan_lock = chan_ref.write().await;
        
        let options = chan_lock.get_options_from_u64(channel_options);
        let original_options = chan_lock.options();
        let chan_id = chan_lock.id();
        let client_name = client_lock.name();

        if client_lock.gettype() as u8 != ClientType::Agent as u8 {

            for mode in ChannelOptions::public_modes().iter() {
                if options as u64 & *mode as u64 == *mode as u64 && !chan_lock.has_option(*mode) {
                    chan_lock.add_option(*mode);
                }
                else if options as u64 & *mode as u64 != *mode as u64 && chan_lock.has_option(*mode) {
                    chan_lock.remove_option(*mode);
                }
            }

            if original_options as u64 == chan_lock.options() as u64 {
                return;
            }
            std::mem::drop(client_lock);
            let chanopt = chan_lock.options_vec_string();
            let _ = chan_lock.sendto_all(&CMessage::new(MessageType::Message("<i class=\"fa fa-gear\"> </i><i class=\"ichat-modechange\"> has changed the channel modes.</i>".to_string()), Uuid::nil(), chan_id, Some(client_name))).await;
            let _ = chan_lock.sendto_all(&CMessage::new(MessageType::ChannelModes(Some(chanopt)), Uuid::nil(), chan_id, None).into()).await;
            return; // end of non agents setting modes
        }

        if original_options as u64 == options as u64 {
            return;
        }

        println!("modes: {} and new is {} -- {}", original_options as u64, options as u64, channel_options as u64);
        chan_lock.set_options(options);
        std::mem::drop(client_lock);
        let chanopt = chan_lock.options_vec_string();
        let _ = chan_lock.sendto_all(&CMessage::new(MessageType::Message("<i class=\"fa fa-gear\"> </i><i class=\"ichat-modechange\"> has changed the channel modes.</i>".to_string()), Uuid::nil(), chan_id, Some(client_name))).await;
        let _ = chan_lock.sendto_all(&CMessage::new(MessageType::ChannelModes(Some(chanopt)), Uuid::nil(), chan_id, None).into()).await;
        return; 
    }

    pub async fn set_channel_topic(server: &Mutex<Server>, client: &RwLock<Client>, channel: Uuid, topic: String, max_len: u16) {
        let chan_ref = server.lock().await.get_channel_ref(channel).await;

        if chan_ref.is_none() {
            let _ = client.write().await.sender().lock().await.send(CMessage::new_problem(ProblemCode::ChannelInvalid, Some(channel), "Cannot change topics of non-existent channels.".to_string()).into()).await;
            return;
        }

        let chan_ref = chan_ref.unwrap();
        let mut client_lock = client.write().await;
        let chan_lock = chan_ref.read().await;
        let chan_owner = chan_lock.owner_id().unwrap_or(Uuid::nil());
        let chan_id = chan_lock.id();
        let client_name = client_lock.name();

        if chan_owner != client_lock.id() && client_lock.gettype() as u8 != ClientType::Agent as u8 {
            let _ = client_lock.sender().lock().await.send(CMessage::new_problem(ProblemCode::PermissionDenied, Some(channel), "You have no permission to change this channels topic.".to_string()).into()).await;
            return;
        } 

        std::mem::drop(chan_lock);
        let mut chan_lock = chan_ref.write().await;
        let clean_topic = topic.replace('<', "&lt;").replace('>', "&gt;");
        let topic_len = clean_topic.char_indices().count();

        if topic_len > max_len as usize {
            let _ = client_lock.sender().lock().await.send(CMessage::new_problem(ProblemCode::InvalidArgument, Some(channel), format!("Topics must be shorter than {} characters.", max_len).to_string()).into()).await;
            return;
        } 

        if clean_topic.char_indices().count() == 0 {
            chan_lock.set_topic(None).await;
        } else {
        chan_lock.set_topic(Some(clean_topic.clone())).await;
        }

        let _ = chan_lock.sendto_all(&CMessage::new(MessageType::Message("<i class=\"ichat-modechange\"> has changed the channel topic.</i>".to_string()), Uuid::nil(), chan_id, Some(client_name))).await;
        let _ = chan_lock.sendto_all(&CMessage::new(MessageType::Topic(clean_topic), Uuid::nil(), chan_id, None)).await;
    }

    pub async fn channel_list(server: &Mutex<Server>, client: &RwLock<Client>) {
        let channels = server.lock().await.get_channels().await;

        if channels.is_empty() {
            let _ = client.write().await.sender().lock().await.send(CMessage::new(MessageType::ChannelList(Vec::new()), Uuid::nil(), Uuid::nil(), None).into()).await;
            return;
        }

        let mut chan_vec: Vec<(Uuid, String, String, u64, u64)> = Vec::new();
        
        for (id, channel) in channels.iter() {
            let clock = channel.read().await;
            let invisible = clock.has_option(ChannelOptions::Invisible);
            let secret = clock.has_option(ChannelOptions::Secret);
            let agentonly = clock.has_option(ChannelOptions::AgentOnly);

            if invisible || secret || agentonly && client.read().await.gettype() as u8 != ClientType::Agent as u8 {
                continue;
            }

            chan_vec.push((*id, clock.name(), clock.topic().unwrap_or("".to_string()), clock.count_members().await, clock.options().bits()));
        }

        let _ = client.write().await.sender().lock().await.send(CMessage::new(MessageType::ChannelList(chan_vec), Uuid::nil(), Uuid::nil(), None).into()).await;
        return;
    }

    /// Handles login attempts using a username and password 
    pub async fn auth_agent(server: &Mutex<Server>, client: &RwLock<Client>, authfinder: Arc<Mutex<Box<dyn AuthFinder + Send + Sync>>>, config: Config, nickname: String, username: String, password: String) {
        let userauth = authfinder.lock().await.by_username_password(&username, &password).await;

        if userauth.is_none() {
            let _= client.write().await.sender().lock().await.send(CMessage::new_problem(ProblemCode::InvalidAuth, None, "The username and password are not valid.".to_string()).into()).await;
            client.write().await.set_status(crate::ClientStatus::Closing);
            server.lock().await.remove_client(client.read().await.id()).await;
            let _ = client.write().await.sender.lock().await.close().await;
            return;
        }

        let userauth = userauth.unwrap();

        if server.lock().await.client_name_to_uuid(&nickname).await.is_some() {
            let _ = client.write().await.sender().lock().await.send(CMessage::new_problem(ProblemCode::NameInUse, None, nickname).into()).await;
            return;
        }

        let mut c_lock = client.write().await;

        if !c_lock.set_name(&nickname).await {
            let _ = c_lock.sender().lock().await.send(CMessage::new_problem(ProblemCode::NameInvalid, None, nickname).into()).await;
            return;
        }

        c_lock.set_status(ClientStatus::Connected);
        c_lock.set_type(ClientType::Agent);
        c_lock.set_options_u64(userauth.permissions);
        server.lock().await.guest_count += 1;
        let id = c_lock.id();
        let name = c_lock.name();
        let _ = c_lock.sender().lock().await.send(CMessage::new(MessageType::AuthOk(id.to_owned(), name.to_owned()), Uuid::nil(), Uuid::nil(), None ).into()).await;
        let motd = server.lock().await.motd_agents.clone();
                
        if motd.len() > 0 {
            let _ = c_lock.sender().lock().await.send(CMessage::new(MessageType::Motd(motd), Uuid::nil(), id, Some("Agent Message of the Day".to_string())).into()).await;
        }

        if config.use_global_lobby {
            if let Some(global_channel) = server.lock().await.get_channel_by_name("Global Lobby".to_string()).await {
                let mut lock = global_channel.write().await;
                println!("*** adding client {} to Global Lobby", 1+lock.count_members().await);
                lock.add_member(c_lock.clone()).await;
                c_lock.add_channel(lock.id(), lock.name()).await;

                if !lock.has_option(ChannelOptions::Invisible) || c_lock.gettype() as u8 == ClientType::Agent as u8 {
                    let _ = c_lock.sender().lock().await.send(CMessage::new(MessageType::Join(name.to_owned(), IpAddr::V4(Ipv4Addr::new(0,0,0,0))), id.to_owned(), lock.id(), Some(lock.name())).into()).await;
                    
                    if !lock.has_option(ChannelOptions::HiddenMemberList) {
                        let mut members_list: Vec<(Uuid, String)> = Vec::new();
                        for (id,cl) in lock.get_members().await.iter() {
                            members_list.push((id.to_owned(), cl.lock().await.name()));
                        }
                        
                        if members_list.is_empty() {
                            let _ = c_lock.sender().lock().await.send(CMessage::new(MessageType::UserList(None), Uuid::nil(), lock.id(), None).into()).await;
                        } else {
                        let _ = c_lock.sender().lock().await.send(CMessage::new(MessageType::UserList(Some(members_list)), Uuid::nil(), lock.id(), None).into()).await;
                        }
                    }

                    if lock.topic().is_some() {
                        let _ = c_lock.sender().lock().await.send(CMessage::new(MessageType::Topic(lock.topic().unwrap()), Uuid::nil(), lock.id(), None).into()).await;
                    }
                }

                lock.to_log(format!("{:?} / JOIN CHANNEL: {} ({}@{})", std::time::SystemTime::now(), lock.name(), c_lock.name(), c_lock.ip())).await;
            }
        }

        if config.use_staff_lobby {
            if let Some(global_channel) = server.lock().await.get_channel_by_name("Staff Lobby".to_string()).await {
                let mut lock = global_channel.write().await;
                lock.add_member(c_lock.clone()).await;
                c_lock.add_channel(lock.id(), lock.name()).await;

                let _ = c_lock.sender().lock().await.send(CMessage::new(MessageType::Join(name.to_owned(), IpAddr::V4(Ipv4Addr::new(0,0,0,0))), id.to_owned(), lock.id(), Some(lock.name())).into()).await;
                
                if !lock.has_option(ChannelOptions::HiddenMemberList) {
                    let mut members_list: Vec<(Uuid, String)> = Vec::new();
                    for (id,cl) in lock.get_members().await.iter() {
                        members_list.push((id.to_owned(), cl.lock().await.name()));
                    }
                    
                    if members_list.is_empty() {
                        let _ = c_lock.sender().lock().await.send(CMessage::new(MessageType::UserList(None), Uuid::nil(), lock.id(), None).into()).await;
                    } else {
                    let _ = c_lock.sender().lock().await.send(CMessage::new(MessageType::UserList(Some(members_list)), Uuid::nil(), lock.id(), None).into()).await;
                    }
                }

                if lock.topic().is_some() {
                    let _ = c_lock.sender().lock().await.send(CMessage::new(MessageType::Topic(lock.topic().unwrap()), Uuid::nil(), lock.id(), None).into()).await;
                }

                lock.to_log(format!("{:?} / JOIN CHANNEL: {} ({}@{})", std::time::SystemTime::now(), lock.name(), c_lock.name(), c_lock.ip())).await;
            }
        }
    }

    pub async fn typing(server: &Mutex<Server>, client: &RwLock<Client>, target: Uuid) {
        let slock = server.lock().await;
        let channel = match slock.get_channel_ref(target).await {
            Some(rchan)   => rchan,
            None        => return
        };
        let hidden_msg = channel.read().await.has_option(ChannelOptions::HiddenMessages);
        let hidden_members = channel.read().await.has_option(ChannelOptions::HiddenMemberList);
        let clientid = client.read().await.id();

        if hidden_msg || hidden_members {
            let _ = channel.write().await.sendto_agents(&CMessage::new(MessageType::Typing(clientid, target), clientid, target, None).into()).await;
            return;
        } else {
            let _ = channel.write().await.sendto_all_butone(clientid, &CMessage::new(MessageType::Typing(clientid, target), clientid, target, None).into()).await;
            return;
        }
        // if !let Some(channel) = match slock.
    }

    pub async fn part(server: &Mutex<Server>, client: &RwLock<Client>, channel: Uuid) {
        if !client.read().await.has_options(ClientOptions::Admin) && !client.read().await.has_options(ClientOptions::PartChannels) {
            let _= client.write().await.sender().lock().await.send(CMessage::new_problem(ProblemCode::PermissionDenied, None, "You may not leave channels".to_string()).into()).await;
            return;
        } 
        
        let slock = server.lock().await;

        let channel_ref = match slock.get_channel_ref(channel).await {
            Some(r)   => r,
            None        => {
                let _= client.write().await.sender().lock().await.send(CMessage::new_problem(ProblemCode::ChannelInvalid, None, "Invalid channel.".to_string()).into()).await;
                return;
            }
        };

        if !channel_ref.read().await.is_member(client.read().await.id()).await {
            let _= client.write().await.sender().lock().await.send(CMessage::new_problem(ProblemCode::NotMember, None, "Cannot leave channels you are not a member of.".to_string()).into()).await;
            return;
        }
        
        let clock = client.read().await;
        let client_type = clock.gettype();
        let client_id = clock.id();
        let client_name = clock.name();
        let client_ip = clock.ip();
        let is_admin = clock.has_options(ClientOptions::Admin);
        std::mem::drop(clock);

        if channel_ref.read().await.has_option(ChannelOptions::CanNotLeave) && (client_type as u64 != ClientType::Agent as u64 || !is_admin) {
            let _= client.write().await.sender().lock().await.send(CMessage::new_problem(ProblemCode::PermissionDenied, None, "You may not leave this channel.".to_string()).into()).await;
            return;
        }

        // we are good to let them leave the channel
        let cmessage = CMessage::new(MessageType::Part(client.read().await.id(), channel, IpAddr::V4(Ipv4Addr::new(0,0,0,0))), client_id, channel, None);
        let _ = client.write().await.sender().lock().await.send(cmessage.into()).await;
        channel_ref.write().await.remove_member(client_id).await;
        channel_ref.write().await.broadcast_part(client_id, client_name.clone(), client_ip).await;

        let is_persist = channel_ref.read().await.has_option(ChannelOptions::Persist);

        if channel_ref.read().await.count_members().await == 0 && !is_persist {
            std::mem::drop(slock); // not mut
            let mut slock = server.lock().await;
            let uuid = channel_ref.read().await.id();

            if channel_ref.read().await.has_option(ChannelOptions::SaveHistory) {
                let logfile = OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(format!("logs/{}.log",channel_ref.read().await.id()));

                if logfile.is_ok() {
                    println!("logfile is ok");
                    let log_entry = format!("{:?} / DESTROY: {} ({}@{})\n", std::time::SystemTime::now(), channel_ref.read().await.name(), client_name , client_ip);

                    let _ = logfile.unwrap().write(log_entry.as_bytes());
                }
            }
            slock.remove_channel(uuid).await;
        } else {
            if channel_ref.read().await.has_option(ChannelOptions::SaveHistory) {
                let logfile = OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(format!("logs/{}.log",channel_ref.read().await.id()));

                if logfile.is_ok() {
                    println!("logfile is ok");
                    let log_entry = format!("{:?} / PART: {} ({}@{})\n", std::time::SystemTime::now(), channel_ref.read().await.name(), client_name , client_ip);

                    let _ = logfile.unwrap().write(log_entry.as_bytes());
                }
            }
        }
    }

    /// process a join channel command. channel can be a uuid or name
    pub async fn join(server: &Mutex<Server>, client: &RwLock<Client>, channel: String, is_uuid: bool) {
        let mut channel_ref: Option<Arc<RwLock<Channel>>> = None;
        let mut slock = server.lock().await;

        if !client.read().await.has_options(ClientOptions::Admin) && !client.read().await.has_options(ClientOptions::JoinChannels) {
            let _= client.write().await.sender().lock().await.send(CMessage::new_problem(ProblemCode::PermissionDenied, None, "You may not create new channels".to_string()).into()).await;
            return;
        }
        if is_uuid {
            if let Some(chan) = slock.get_channel_ref(Uuid::from_str(&channel).unwrap()).await {
            channel_ref = Some(chan);
            }
        } else {
            if let Some(chan_uuid) = slock.channel_name_to_uuid(channel.clone()).await {
                if let Some(chan) = slock.get_channel_ref(chan_uuid).await {
                    channel_ref = Some(chan);
                }

            }
        }
        // channel exists, treat it based on its settings
        if channel_ref.is_some() {
            let channel_ref = channel_ref.unwrap();
            let cref_read = channel_ref.read().await;

            if cref_read.is_member(client.read().await.id()).await {
                let _= client.write().await.sender().lock().await.send(CMessage::new_problem(ProblemCode::AlreadyMember, None, "You are already a member of that channel.".to_string()).into()).await;
                return;
            }
            if cref_read.has_option(ChannelOptions::AgentOnly) && client.read().await.gettype() as u64 != ClientType::Agent as u64 {
                let _= client.write().await.sender().lock().await.send(CMessage::new_problem(ProblemCode::PermissionDenied, None, "Channel restricted to Agents.".to_string()).into()).await;
                return;
            }

            if cref_read.has_option(ChannelOptions::InviteOnly) && 
                !client.read().await.has_options(ClientOptions::Admin) &&
                !cref_read.has_invite(client.read().await.id()) {
                    let _= client.write().await.sender().lock().await.send(CMessage::new_problem(ProblemCode::PermissionDenied, None, "Channel is invite only.".to_string()).into()).await;
                    return;
            } 

            // client is allowed to join the channel at this point
            std::mem::drop(cref_read);
            let mut cref_write = channel_ref.write().await;

            let mut clock = client.write().await;
            cref_write.add_member(clock.clone()).await;
            clock.add_channel(cref_write.id(), cref_write.name()).await;
            // cref_write.add_member(client.write().await.clone()).await;
            let username = clock.name();
            let userid = clock.id();

            if cref_write.has_option(ChannelOptions::SaveHistory) {
                let logfile = OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(format!("logs/{}.log",cref_write.id()));

                if logfile.is_ok() {
                    println!("logfile is ok");
                    let log_entry = format!("{:?} / JOIN: {} ({}@{})\n", std::time::SystemTime::now(), cref_write.name(), clock.name(), clock.ip());

                    let _ = logfile.unwrap().write(log_entry.as_bytes());
                }
            }

            if !cref_write.has_option(ChannelOptions::Invisible) || clock.gettype() as u8 == ClientType::Agent as u8 {
                let _ = clock.sender().lock().await
                    .send(CMessage::new(MessageType::Join(username.to_owned(), 
                        IpAddr::V4(Ipv4Addr::new(0,0,0,0))), userid,  cref_write.id(), Some(cref_write.name())).into()).await;

                // send the user list if applicable
                if !cref_write.has_option(ChannelOptions::HiddenMemberList) || clock.gettype() as u64 == ClientType::Agent as u64 {
                    let mut members_list: Vec<(Uuid, String)> = Vec::new();

                    for (id,cl) in cref_write.get_members().await.iter() {
                        members_list.push((id.to_owned(), cl.lock().await.name()));
                    }
                    
                    if members_list.is_empty() {
                        let _ = clock.sender().lock().await.send(CMessage::new(MessageType::UserList(None), Uuid::nil(), cref_write.id(), None).into()).await;
                    } else {
                    let _ = clock.sender().lock().await.send(CMessage::new(MessageType::UserList(Some(members_list)), Uuid::nil(), cref_write.id(), None).into()).await;
                    }

                }
            }

        } 
        else {
            let mut clock = client.write().await;

            if !clock.has_options(ClientOptions::CreateChannels) && clock.gettype() as u64 != ClientType::Agent as u64 {
                let _ = clock.sender().lock().await.send(CMessage::new_problem(ProblemCode::PermissionDenied, None, "You may not create channels.".to_string()).into()).await;
                return;
            }
           
            // safety safety, dippity dooo.
            let regexor = regex::Regex::new(&"^[a-zA-Z0-9- ]{3,50}$").unwrap();
            if regex::Regex::new("^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}").unwrap().is_match(&channel) {
                let _ = clock.sender().lock().await.send(CMessage::new_problem(ProblemCode::ChannelInvalid, None, "Channel does not exist.".to_string()).into()).await;
                return;
            }

            if !regexor.is_match(&channel) {
                let _ = clock.sender().lock().await.send(CMessage::new_problem(ProblemCode::ChannelNameBad, None, "Channels may not contain non a-z0-9 characters.".to_string()).into()).await;
                return;

            }

            let mut chan_creator = Channel::new(channel.to_string(), Some(clock.id()), false);
            chan_creator.set_options(
                ChannelOptions::ClientInvites.or(ChannelOptions::SaveHistory)

            );

            let username = clock.name();
            let userid = clock.id();
            chan_creator.add_member(clock.clone()).await;
            clock.add_channel(chan_creator.id(), chan_creator.name()).await;

            let _ = clock.sender().lock().await
                .send(CMessage::new(MessageType::Join(username, 
                IpAddr::V4(Ipv4Addr::new(0,0,0,0))), userid,  chan_creator.id(), Some(chan_creator.name())).into()).await;

            if chan_creator.has_option(ChannelOptions::SaveHistory) {
                let logfile = OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(format!("logs/{}.log",chan_creator.id()));

                if logfile.is_ok() {
                    println!("logfile is ok");
                    let log_entry = format!("{:?} / CREATE CHANNEL: {} ({}@{})\n", std::time::SystemTime::now(), chan_creator.name(), clock.name(), clock.ip());

                    let _ = logfile.unwrap().write(log_entry.as_bytes());
                }
            }

            slock.add_channel(chan_creator).await;
            
        }

    }

}
#[repr(u8)]
pub enum CommandError {
    None,           // defaults to 0, in theory we dont use this
    BadRequest,     // the server just has no idea
    AuthFirst,      // the client must auth before using commands
    ClientInvalid,  // client target was not found
    ChannelInvalid, // channel target was not found
    FileInvalid,    // file for download was not found
    SizeExceeded,   // the message size is too large
    NoPermissions,  // no permissions for the requested command.
    InviteOnly,     // channel is invite only 

}
