use std::borrow::BorrowMut;
use std::collections::HashMap;
use std::{net::IpAddr, time::SystemTime};
use std::sync::Arc;
use crate::{config, channel::*, message::Message as CMessage};
use crate::Uuid;
use futures_util::stream::SplitSink;
use rand::Rng;
use tokio::net::TcpStream;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Mutex;
// use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::Message as Message;
use tokio_tungstenite::WebSocketStream;
use regex::Regex;


#[derive(Clone)]
pub struct Client {
    id: Uuid,                   // internal id
    pub sender: Arc<Mutex<SplitSink<WebSocketStream<tokio_rustls::server::TlsStream<TcpStream>>, Message>>>, // physical sender
    pub main_sender: UnboundedSender<CMessage>,
    name: String,               // display name of the channel
    r#type: ClientType,         // agent / guest / contact
    options: ClientOptions,               // ClientOptions bitset
    ip: IpAddr,
    connected: SystemTime,
    last_ping: SystemTime,
    last_action: SystemTime,
    status: ClientStatus,
    channels: Arc<Mutex<HashMap<Uuid, String>>> // uuid and channel name
}

#[repr(u64)]
#[derive(Clone, Copy)]
pub enum ClientOptions {
    None            = 0 << 0,
    Admin           = 1 << 1,
    JoinChannels    = 1 << 2, // client can join channels
    PartChannels    = 1 << 3, // client can leave channels
    CreateChannels  = 1 << 4,
    CanInvite       = 1 << 5, // applies only to invitable channels.
    FilesAllowed    = 1 << 6, // can upload files in general.
    Invisible       = 1 << 7, // can join/participate in chats but wont show in users list.
}

#[derive(Clone)]
pub enum ClientStatus {
    PendingAuth,
    Connected,
    Zombie, // disconnected but maybe just page reload ?
    Closing,        // connection is closing, dont send anything
    // Disconnected    // connection is closed
}

#[repr(u8)]
#[derive(Copy, Clone)]
pub enum ClientType {
    None,
    Agent,
    Contact,
    Guest
}

impl ClientOptions {
    pub fn or(self, rhs: ClientOptions) -> ClientOptions {
        unsafe { std::mem::transmute::<u64, ClientOptions>((self as u64) | (rhs as u64)) }
    }

    pub fn and(self, rhs: ClientOptions) -> ClientOptions {
        unsafe { std::mem::transmute::<u64, ClientOptions>((self as u64) & (rhs as u64)) }
    }

    pub fn xor(self, rhs: ClientOptions) -> ClientOptions {
        unsafe { std::mem::transmute::<u64, ClientOptions>((self as u64) ^ (rhs as u64)) }
    }

    pub fn bits(self) -> u64 {
        self as u64
    }

    pub fn from_u64(bits: u64) -> ClientOptions {
        unsafe { std::mem::transmute::<u64, ClientOptions>(bits)}
    }
}

impl Client {
    pub fn has_options(&self, opt: ClientOptions) -> bool {
        self.options.and(opt) as u64 == opt as u64
    }
    pub fn generate_id() -> Uuid {
        Uuid::new_v4()
    }

    pub fn generate_guest_name(id: Option<u64>) -> String {
        if id.is_none() {
            format!("Guest-{}", rand::thread_rng().gen_range(122345..999999)) 
        } else {
            format!("Guest-{}", id.unwrap())
        }
    }

    pub fn new(sender: SplitSink<WebSocketStream<tokio_rustls::server::TlsStream<TcpStream>>, Message>, main_sender: UnboundedSender<CMessage>, ip: IpAddr, guestid: Option<u64>) -> Self {
        Client {
            id: Client::generate_id(),
            sender: Arc::new(Mutex::new(sender)),
            main_sender,
            name: Client::generate_guest_name(guestid),
            r#type: ClientType::None,
            options: ClientOptions::JoinChannels,
            ip,
            connected: SystemTime::now(),
            last_ping: SystemTime::now(),
            last_action: SystemTime::now(),
            status: ClientStatus::PendingAuth,
            channels: Arc::new(Mutex::new(HashMap::new()))
        }
    }

    pub async fn add_channel(&mut self, id: Uuid, name: String) {
        self.channels.lock().await.insert(id, name);
    }

    pub async fn remove_channel(&mut self, id: Uuid) {
        self.channels.lock().await.remove(&id);
    }

    pub async fn in_channel(&self, id: Uuid) -> bool {
        self.channels.lock().await.contains_key(&id)
    }

    /// Clear this clients channel list (local)
    ///
    /// This function clears the local channel list relative to the client.
    /// It does NOT actually remove their channel memberships, and may still
    /// receive messages, but may run into other issues with sync. You should
    /// run remove_user() on the channels listed before using this.
    pub async fn clear_channel_list(&mut self) {
        self.channels.lock().await.clear();
    }

    pub fn connected_time(&self) -> SystemTime {
        self.connected
    }
     
    pub fn last_ping_time(&self) -> SystemTime {
        self.last_ping
    }

    pub fn update_last_ping_time(&mut self) {
        self.last_ping = SystemTime::now();
    }

    pub fn update_last_action_time(&mut self) {
        self.last_action = SystemTime::now();
    }

    pub fn last_action(&self) -> SystemTime {
        self.last_action
    }
    
    /// Clears our clients channel listing.
    ///
    /// This will not remove clients from the channel model, this
    /// is only for clearing the local clients channel list.
    pub async fn clear_channels(&mut self) {
        self.channels.lock().await.clear();
    }

    pub async fn channels(&self) -> HashMap<Uuid, String> {
        self.channels.lock().await.clone()
    }

    /// ANYTHING BELOW SHOULD BE FOR FIELD GETTERS
    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn ip(&self) -> IpAddr {
        self.ip
    }

    pub fn options(&self) -> ClientOptions {
        self.options.clone()
    }

    pub fn set_options_u64(&mut self, options: u64) {
        self.options = ClientOptions::from_u64(options);
    }

    pub fn gettype(&self) -> ClientType {
        self.r#type
    }

    pub fn set_type(&mut self, r#type: ClientType) {
        self.r#type = r#type;
    }

    pub fn set_status(&mut self, status: ClientStatus) {
        self.status = status;
    }

    pub fn status(&self) -> ClientStatus {
        self.status.clone()
    }

    pub fn name(&self) -> String {
        self.name.to_string()
    }

    pub async fn set_name(&mut self, name: &String) -> bool {
        let re = Regex::new(r"^[a-zA-Z0-9 \-]{3,30}$").expect("unable to create regex");

        if !re.is_match(&name) {
            return false;
        }

        self.name = name.to_owned();
        true
    }

    pub fn sender(&mut self) -> &mut Arc<Mutex<SplitSink<WebSocketStream<tokio_rustls::server::TlsStream<TcpStream>>, Message>>> {
        self.sender.borrow_mut()
    }
}
