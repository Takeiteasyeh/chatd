
use serde::{Deserialize, Serialize};
use tokio_tungstenite::tungstenite::Message as TMessage;
use crate::{Uuid, client::*, config::*, channel::*, commands::* };
use std::{net::IpAddr, time::SystemTime};

#[derive(Serialize, Deserialize, Clone)]
pub struct Message {
    pub id: u64,
    pub r#type: MessageType,
    pub source: Uuid,
    pub target: Uuid,
    pub message: String
}

#[derive(Serialize, Deserialize, Clone)]
pub enum MessageType {
    AuthGuest(Option<String>),  // username
    AuthDevice(String, String), // deviceuuid, username
    AuthAgent(String, String, String), // nickname, username, password
    AuthOk(Uuid, String), // accepted uuid and username
    Ping(u64), // ping with current systemtime
    Pong(u64), // reply, sending back the systemtime from PING
    // client name, client ip, send channel name if source is current client in message
    Join(String, IpAddr),      //  client name, client ip, name sent in message on first join
    Part(Uuid, Uuid, IpAddr),
    Kick(Uuid, Uuid, String),
    ChannelModes(Option<Vec<String>>),
    SetChannelModes(u64),
    Quit(String), // quit reason or message
    Kill(String, String), // kicker, reason -- use target in message
    Kline(String, u64, String), // ip, expiry in seconds, reason
    Whois(),
    Message(String),
    Motd(String),
    Topic(String),
    Private(Uuid, Uuid, String),
    File,
    Typing(Uuid, Uuid),
    Users(Uuid),
    UserList(Option<Vec<(Uuid,String)>>),
    Channels,
    ChannelList(Vec<(Uuid, String, String, u64, u64)>), // id, name, channel topic usercount, channel modes
    Wall(String), // message to all connections
    Wallop(String), // message to agent connections
    Walladmin(String), // message to all admins
    Problem(ProblemCode) // code 
}

impl From<Message> for TMessage {
    fn from(data: Message) -> Self {
        let json = serde_json::to_string(&data).expect("unable to serialize CMessage to Message");
        TMessage::text(json)
    }
}

impl Message {
    pub fn new(r#type: MessageType, source: Uuid, target: Uuid, message: Option<String>) -> Self {
        Message {
            id: rand::random::<u64>(),
            r#type,
            source,
            target,
            message: if message.is_some() { message.unwrap() } else { "".to_string() }
        }
    }

    pub fn new_problem(problem: ProblemCode, target: Option<Uuid>, message: String) -> Self {
        let target = target.unwrap_or_else(|| Uuid::nil());
        Message {
            id: rand::random::<u64>(),
            r#type: MessageType::Problem(problem), 
            source: Uuid::nil(),
            target,
            message
        }
    }

    pub fn sanitize_text_message(mut message: String) -> String {
        message = message.replace("<", "&lt;");
        message = message.replace(">", "&gt;");

        message
    }
}
pub trait MessageSendable {
    async fn sendto_one(&mut self, to: Uuid, message: &Message) -> Result<u64, String>;
    async fn sendto_all(&mut self, message: &Message) -> Result<u64, String>;
    async fn sendto_all_butone(&mut self, not: Uuid, message: &Message) -> Result<u64, String>;
    // fn sendto_contacts(&self, message: &Message) -> Result<u64, String>;
    async fn sendto_agents(&mut self, message: &Message) -> Result<u64, String>;
    async fn sendto_nonagents(&mut self, message: &Message) -> Result<u64, String>;
    // fn sendto_guests(&self, message: &Message) -> Result<u64, String>;
}

#[repr(u64)]
#[derive(Clone, Deserialize, Serialize)]
pub enum ProblemCode {
    NameInUse,          // name is already in use
    NameInvalid,        // name is not valid to be used
    InvalidAuth,        // auth us not valid for login
    InvalidArgument,    // an argument wasnt valid
    NotAvailable,       // not available for some reason
    PermissionDenied,
    AlreadyMember,      // already a member of a channel
    NotMember,          // not a member of a channel
    ChannelNameBad,     // channel name has bad chars
    ChannelInvalid,     // channel doesnt exist.
    KickedFromServer,   // kicked from the server
}
