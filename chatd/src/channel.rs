use futures_util::SinkExt;
// use rustls::pki_types::Ipv4Addr;
use std::net::{IpAddr, Ipv4Addr};
use crate::client::*;
use crate::message::{Message as CMessage, *};
use crate::Message;
use crate::Uuid;
use crate::Mutex;
use crate::Arc;
use crate::{Write, OpenOptions};
use std::collections::HashMap;


pub struct Channel {
    id: Uuid,
    name: String,
    owner: Option<Uuid>,
    clientid: Option<u64>,
    members: HashMap<Uuid, Arc<Mutex<Client>>>,
    zombies: HashMap<String, Client>, // zombies are stored by username
    private: bool, // usually agent to agent
    topic: Option<String>,
    options: ChannelOptions
}

#[repr(u64)]
#[derive(Clone, Copy)]
pub enum ChannelOptions {
    None            = 0 << 0,
    ClientInvites   = 1 << 0, // allow client to invite others 
    AgentOnly       = 1 << 1, // only agents can join the channel
    InviteOnly      = 1 << 2, // only creator or admins+ can join
    SaveHistory     = 1 << 3, // save channel history when channel members = 0
    Persist         = 1 << 4, // persist channel even if members = 0
    WaitForAgent    = 1 << 5, // client can send messages but must wait for agent otherwise
    RejoinClients   = 1 << 6, // disconnected clients are automatically rejoined on connect
    CanNotLeave     = 1 << 7, // members may not leave the channel
    HiddenMemberList= 1 << 8, // member list hidden from non agents
    HiddenMessages  = 1 << 9, // messages are hidden from non agents
    Invisible       = 1 << 10, //channel will not show up as active to non agents
    Secret          = 1 << 11, // channel will not show up in channel listings to non agents

    // AntiFlood    = 1 << 11, // flooding will get the user banned
    // Throttle     = 1 << 12, // slow down user messages to once every 30 seconds
    // Limit        = 1 << 13, // increases channel user limits by 5 every 30 seconds
}
impl ChannelOptions {
    pub fn to_string(&self) -> String {
        match self {
            Self::None              => "",
            Self::ClientInvites     => "Allow Invites",
            Self::AgentOnly         => "Agent Only",
            Self::InviteOnly        => "Invite Only",
            Self::SaveHistory       => "Save History",
            Self::Persist           => "Persist Empty",
            Self::WaitForAgent      => "Wait for Agent",
            Self::RejoinClients     => "Rejoin on Disconnect",
            Self::CanNotLeave       => "May Not Leave",
            Self::HiddenMemberList  => "Hidden Members",
            Self::HiddenMessages    => "Hidden Messages",
            Self::Invisible         => "Invisible",
            Self::Secret            => "Not Listed"
            // _                       => ""
        }.to_string()
    }

    pub fn public_modes() -> Vec<ChannelOptions> {
        vec![
            Self::InviteOnly,
            Self::Secret
        ]
    }

    pub fn available() -> Vec<ChannelOptions> {
        vec![
            Self::ClientInvites, 
            Self::AgentOnly,
            Self::InviteOnly,
            Self::SaveHistory,
            Self::Persist,
            Self::WaitForAgent,
            Self::RejoinClients,
            Self::CanNotLeave,
            Self::HiddenMemberList,
            Self::HiddenMessages,
            Self::Invisible,
            Self::Secret
        ]
    }

    pub fn or(self, rhs: ChannelOptions) -> ChannelOptions {
        unsafe { std::mem::transmute::<u64, ChannelOptions>((self as u64) | (rhs as u64)) }
    }

    pub fn and(self, rhs: ChannelOptions) -> ChannelOptions {
        unsafe { std::mem::transmute::<u64, ChannelOptions>((self as u64) & (rhs as u64)) }
    }

    pub fn xor(self, rhs: ChannelOptions) -> ChannelOptions {
        unsafe { std::mem::transmute::<u64, ChannelOptions>((self as u64) ^ (rhs as u64)) }
    }

    pub fn bits(self) -> u64 {
        self as u64
    }
    
}

impl Channel {
    pub fn new(name: String, owner: Option<Uuid>, private: bool) -> Self {
        let options: ChannelOptions;

        if private { 
            options = ChannelOptions::RejoinClients
                .or(ChannelOptions::Persist)
                .or(ChannelOptions::AgentOnly);
        } else { 
            options = ChannelOptions::ClientInvites
                .or(ChannelOptions::SaveHistory)
                .or(ChannelOptions::WaitForAgent)
                .or(ChannelOptions::RejoinClients);
        }

        Channel {
            id: Uuid::new_v4(), name, owner, private,
            clientid: None,
            members: HashMap::new(),
            // members: Arc::new(Mutex::new(HashMap::new())),
            zombies: HashMap::new(),
            topic: None,
            options
        }
        
    }

    pub fn get_options_from_u64(&mut self, modes: u64) -> ChannelOptions {
       let mut options: ChannelOptions = ChannelOptions::None;
        for option in ChannelOptions::available().iter() {
            if modes & *option as u64 == *option as u64 {
                options = options.or(*option);
            }
        }
        options
    }

    pub fn add_option(&mut self, chanopt: ChannelOptions) {
        self.options = self.options.or(chanopt);
    }

    pub fn remove_option(&mut self, chanopt: ChannelOptions) {
        self.options = self.options.xor(chanopt);
    }

    pub fn has_option(&self, chanopt: ChannelOptions) -> bool {
        self.options.and(chanopt) as u64 == chanopt as u64
    }

    pub fn options(&self) -> ChannelOptions {
        self.options
    }

    pub fn set_options(&mut self, chanopt: ChannelOptions) {
        self.options = chanopt;
    }

    pub fn owner_id(&self) -> Option<Uuid> {
        self.owner
    }
    pub fn has_invite(&self, user: Uuid) -> bool {
        false
    }

    /// Add a member to the current channel
    ///
    /// Function will broadcast the join to others members as required, as well as
    /// sending the current channel modes, but not the userlist.
    pub async fn add_member(&mut self, mut client: Client) {
        // we never broadcast invisible people. use this power wisely.
        if client.options().and(ClientOptions::Invisible) as u64 == 0 {
            self.broadcast_join(client.id(), client.name(), client.ip()).await;
            
            let _ = client.sender().lock().await.send(CMessage::new(MessageType::ChannelModes(Some(self.options_vec_string())), Uuid::nil(), self.id(), None).into()).await;
        }
        let id = client.id(); // old 129 below
        self.members.insert(id, Arc::new(Mutex::new(client)));
    }

    /// Broadcast a client join to members of a channel as necessary.
    ///
    /// This function will honor Invisible and other options
    async fn broadcast_join(&mut self, client: Uuid, name: String, ip: IpAddr) {

        let agent_only = ChannelOptions::HiddenMemberList.or(ChannelOptions::Invisible);
        if self.options.and(agent_only) as u64 != 0 {
            let cmessage = CMessage::new(MessageType::Join(name, ip), client, self.id(), None);
            let _ = self.sendto_agents(&cmessage).await;
        } else {
            let mut cmessage = CMessage::new(MessageType::Join(name.clone(), IpAddr::V4(Ipv4Addr::new(0,0,0,0))), client, self.id(), None);
            let _ = self.sendto_nonagents(&cmessage).await;
            cmessage = CMessage::new(MessageType::Join(name, ip), client, self.id(), None);
            let _ = self.sendto_agents(&cmessage).await;

        }
    }

    pub async fn broadcast_part(&mut self, client: Uuid, name: String, ip: IpAddr) {
        let agent_only = ChannelOptions::HiddenMemberList.or(ChannelOptions::Invisible);

        if self.options.and(agent_only) as u64 != 0 {
            let cmessage = CMessage::new(MessageType::Part(client, self.id, ip), client, self.id(), Some(name));
            let _ = self.sendto_agents(&cmessage);
        } else {
            let mut cmessage = CMessage::new(MessageType::Part(client, self.id, IpAddr::V4(Ipv4Addr::new(0,0,0,0))), client, self.id(), Some(name.clone()));
            let _ = self.sendto_nonagents(&cmessage).await;
            cmessage = CMessage::new(MessageType::Part(client, self.id, ip), client, self.id(), Some(name));
            let _ = self.sendto_agents(&cmessage).await;
        }
    }

    /// Remove member from the channel list.
    ///
    /// This function does not broadcast to others
    pub async fn remove_member(&mut self, id: Uuid) {
        self.members.remove(&id);
    }

    pub async fn is_member(&self, id: Uuid) -> bool {
        self.members.get(&id).is_some() 
    }

    pub async fn get_members(&self) -> HashMap<Uuid, Arc<Mutex<Client>>> {
        self.members.clone()
    }
    
    pub async fn count_members(&self) -> u64 {
        self.members.len() as u64
    }

    pub async fn to_log(&self, message: String) {
        if self.has_option(ChannelOptions::SaveHistory) {
            let logfile = OpenOptions::new()
                .append(true)
                .create(true)
                .open(format!("logs/{}.log", self.id));

            if logfile.is_ok() {
                let _ = logfile.unwrap().write(format!("{}\n", message).as_bytes());
            }
        }
    }

    pub async fn count_guests(&self) -> u64 {
        let mut count = 0u64;

        for member in self.members.values() {
            if member.lock().await.gettype() as u64 == ClientType::Guest as u64 {
                count += 1;
            }
        }
        count
    }

    pub async fn count_contacts(&self) -> u64 {
        let mut count = 0u64;

        for member in self.members.values() {
            if member.lock().await.gettype() as u64 == ClientType::Contact as u64 {
                count += 1;
            }
        }
        count
    }

    pub async fn count_agents(&self) -> u64 {
        let mut count = 0u64;

        for member in self.members.values() {
            if member.lock().await.gettype() as u64 == ClientType::Agent as u64 {
                count += 1;
            }
        }
        count
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn topic(&self) -> Option<String> {
        self.topic.clone()
    }

    pub async fn set_topic(&mut self, topic: Option<String>) {
        self.topic = topic;
    }

    pub fn name(&self) -> String {
        self.name.to_string()
    }

    /// Returns a vector of set options for this channel
    pub fn options_vec_string(&self) -> Vec<String> {
        let options = vec![
            ChannelOptions::ClientInvites,
            ChannelOptions::AgentOnly,
            ChannelOptions::InviteOnly,
            ChannelOptions::SaveHistory,
            ChannelOptions::Persist,
            ChannelOptions::WaitForAgent,
            ChannelOptions::RejoinClients,
            ChannelOptions::CanNotLeave,
            ChannelOptions::HiddenMemberList,
            ChannelOptions::HiddenMessages,
            ChannelOptions::Invisible,
            ChannelOptions::Secret,
        ];
        options.iter().filter(|o| self.has_option(**o)).map(|o| o.to_string()).collect()
    }
}

impl MessageSendable for Channel {
    /// Send a message to one member of a channel.
    ///
    /// This function ignores any option and will always send to a matching member.
    async fn sendto_one(&mut self, to: Uuid, message: &CMessage) -> Result<u64, String> {
        let json = match serde_json::to_string(message) {
            Ok(text)    => text,
            Err(e)      => { println!("error: sendto_one() failed on serialization: {}", e); return Err(e.to_string()); }
        };

        for member in self.members.values() { 
            let mut mut_mem = member.lock().await;

            if mut_mem.id() == to {
                match mut_mem.sender().lock().await.send(Message::text(json.clone())).await {
                    Ok(_)   => { },
                    Err(_)  => { }
                }
            }
        }
        Ok(1)
    }

    /// Send a message to all channel members, but one.
    ///
    /// This function ignores any options and will always send to any not matching.
    async fn sendto_all_butone(&mut self, not: Uuid, message: &CMessage) -> Result<u64, String> {
        let json = match serde_json::to_string(message) {
            Ok(text)    => text,
            Err(e)      => { println!("error: sendto_one() failed on serialization: {}", e); return Err(e.to_string()); }
        };

        for member in self.members.values() { 
            let mut mut_mem = member.lock().await;

            if mut_mem.id() != not {
                match mut_mem.sender().lock().await.send(Message::text(json.clone())).await {
                    Ok(_)   => {  },
                    Err(_)  => {  }
                }
            }
        }

        Ok(1)
    }

    /// Send a message to all non-agents of a channel.
    ///
    /// This function will ignore any options and will always send to non-agent members.
    async fn sendto_nonagents(&mut self, message: &CMessage) -> Result<u64, String> {
     let json = match serde_json::to_string(message) {
            Ok(text)    => text,
            Err(e)      => { println!("error: sendto_one() failed on serialization: {}", e); return Err(e.to_string()); }
        };

        for member in self.members.clone().values() { 
            let mut mut_mem = member.lock().await;

            if mut_mem.gettype() as u64 != ClientType::Agent as u64 {
                let _ = mut_mem.sender().lock().await.send(Message::text(json.clone())).await;
            }
        }

        Ok(1)

    }

    /// Send a message to all agents in a channel.
    ///
    /// This function will ignore any options and will always send to agent members.
    async fn sendto_agents(&mut self, message: &CMessage) -> Result<u64, String> {
     let json = match serde_json::to_string(message) {
            Ok(text)    => text,
            Err(e)      => { println!("error: sendto_one() failed on serialization: {}", e); return Err(e.to_string()); }
        };

        for member in self.members.values() { 
            let mut mut_mem = member.lock().await;

            if mut_mem.gettype() as u64 == ClientType::Agent as u64 {
               let _ = mut_mem.sender().lock().await.send(Message::text(json.clone())).await;
            }
        }

        Ok(1)

    }

    /// Send a message to all members of a channel.
    ///
    /// This function will ignore any options and will always send to any members.
    async fn sendto_all(&mut self, message: &CMessage) -> Result<u64, String> {
        let json = match serde_json::to_string(message) {
            Ok(text)    => text,
            Err(e)      => { println!("error: sendto_one() failed on serialization: {}", e); return Err(e.to_string()); }
        };

        for member in self.members.values() { 
            let mut mut_mem = member.lock().await;
            let mut sender_lock = mut_mem.sender().lock().await;
            let _ = sender_lock.send(Message::text(json.clone())).await;
        } 

        Ok(1)
    }

    // fn sendto_contacts(&self, message: &Message) -> Result<u64, String> {
    //     let count = self.members.values().filter(|client| client.gettype() as u64 == ClientType::Contact as u64)
    //         .map(|client| client.sender().send(message.clone())).count() as u64;
    //     Ok(count)
    // }
}
