use crate::client::*;
use crate::channel::*;
use crate::Ban;
use crate::config::Config;
use crate::SystemTime;
use crate::IpAddr;
// use crate::handle_client_error;
use crate::message::Message as CMessage;
use crate::AuthFinder;
use crate::VERSION;
use std::collections::HashMap;
use std::io::Read;
use std::sync::Arc;
use futures_util::SinkExt;
// use std::sync::RwLock;
use std::fs::File;
// use std::io::prelude;
use tokio::sync::RwLock;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

pub struct Server {
    pub receiver: mpsc::UnboundedReceiver<CMessage>,
    clients: Arc<Mutex<HashMap<Uuid, Arc<RwLock<Client>>>>>,
    channels: Arc<Mutex<HashMap<Uuid, Arc<RwLock<Channel>>>>>,
    bans: Arc<RwLock<Vec<Ban>>>,
    // pub authfinder: Arc<Mutex<Box<dyn AuthFinder>>>,
    pub connections_since_start: u64,
    pub invalid_connects: u64, 
    pub banned_connects: u64,
    pub guest_count: u64,
    pub motd_guests: String,
    pub motd_clients: String,
    pub motd_agents: String,
    pub conf: Config
}

impl Server {
    pub fn new(receiver: mpsc::UnboundedReceiver<CMessage>, config: Config) -> Self {
        Server {
            receiver,
            clients: Arc::new(Mutex::new(HashMap::new())),
            channels: Arc::new(Mutex::new(HashMap::new())),
            bans: Arc::new(RwLock::new(Server::load_bans_db(&config.ban_db))),
            connections_since_start: 0,
            invalid_connects: 0,
            banned_connects: 0,
            guest_count: 0,
            motd_guests: Server::load_motd_file(&config.motd_file_guests),
            motd_clients: Server::load_motd_file(&config.motd_file_clients),
            motd_agents: Server::load_motd_file(&config.motd_file_agents),
            conf: config 
        }
    }

    pub fn bans(&self) -> Arc<RwLock<Vec<Ban>>> {
        self.bans.clone()
    }

    pub fn config(&self) -> Config {
        self.conf.to_owned()
    }

    pub async fn count_bans(&self) -> usize {
        self.bans.read().await.len()
    }
    pub async fn add_ban(&mut self, ip: IpAddr, reason: &str, expires: SystemTime, added_by: &str) -> Result<(), String> {

        if Ban::exists(&self.bans, ip).await {
            return Err("the ip already exists in the ban list.".into());
        }

        let mut lock = self.bans.write().await;
        lock.push(Ban::new(ip, reason.into(), expires, added_by.into()));

        Ok(())
    }

    pub async fn remove_ban(&mut self, ip: IpAddr) -> Result<(), String> {
        if !Ban::exists(&self.bans, ip).await {
            return Err("the ip is not in the ban-list".to_string());
        }

        let mut lock = self.bans.write().await;
        lock.retain(|ban| ban.ip != ip);
        Ok(())
    }

    pub async fn ban_exists(&mut self, ip: IpAddr) -> bool {
        Ban::exists(&self.bans, ip).await
    }

    pub async fn save_ban_db(&mut self, filename: &str) -> Result<(), String> {
        if Ban::save_to_disk(filename, self.bans.read().await.clone().as_ref()).is_ok() {
            return Ok(())
        }

        Err("unable to save bans to database".into())
    }

    pub fn load_bans_db(filename: &str) -> Vec<Ban> {
        if let Ok(bans) = Ban::load_bans(filename) {
            return bans;
        }

        println!("error: failed to load bans from {}",  filename);
        Vec::new()
    }

    pub fn load_motd_file(filename: &String) -> String {
        let mut fhandle = match File::open(filename) {
            Ok(h)   => h,
            Err(e)  => { println!("motd error: {} [{}]", filename, e); return "".to_string(); }
        };

        let mut motd_text = String::new();
        
        match fhandle.read_to_string(&mut motd_text) {
            Ok(_)   => return motd_text,
            Err(_)  => return "".to_string()
        }
    }
    pub async fn client_count(&self) -> u64 {
        self.clients.lock().await.len() as u64
    }
    pub async fn add_client(&mut self, client: Client) {
        let mut lock = self.clients.lock().await;
        let id = client.id();
        println!("adding to client");
        lock.insert(id, Arc::new(RwLock::new(client)));
        println!("addint to client ok");
    }

    pub async fn remove_client(&mut self, id: Uuid) {
        let mut lock = self.clients.lock().await;
        lock.remove(&id);
    }

    pub async fn client_exists(&self, id: Uuid) -> bool {
        let lock = self.clients.lock().await;
        lock.get(&id).is_some()
    }

    pub async fn client_name_to_uuid(&self, name: &String) -> Option<Uuid> {
        for client in self.clients.lock().await.values() {
            let lock = client.read().await;
            if lock.name() == *name {
                return Some(lock.id())
            }
        }
        None
    }

    pub async fn client_uuid_to_name(&self, id: Uuid) -> Option<String> {
        for (uid,client) in self.clients.lock().await.iter() {
            if *uid == id {
                return Some(client.read().await.name());
            }
        }
        None
    }

    pub async fn get_clients(&self) -> Option<Arc<Mutex<HashMap<Uuid, Arc<RwLock<Client>>>>>> {
        if self.clients.lock().await.is_empty() {
            return None
        }
        Some(self.clients.clone())
        
    }

    pub async fn get_client_ref(&self, id: Uuid) -> Option<Arc<RwLock<Client>>> {
        match self.clients.lock().await.get(&id) {
            None    => None,
            Some(client)   => Some(client.clone())
        }
    }

    pub async fn sendto_wallops(&mut self, message: CMessage) -> u64 {
        let json = match serde_json::to_string(&message) {
            Ok(j)   => j,
            Err(e)  => { println!("*** failed sendto_wallops(): unable to serialize CMessage [{}]", e); return 0 }
        };
        let mut count = 0u64;

        for client in self.clients.lock().await.values() {
            if client.read().await.status() as u64 == ClientStatus::Connected as u64 {
                if client.read().await.gettype() as u64 != ClientType::Agent as u64 {
                    continue;
                }

                let mut client_lock = client.write().await;

                if client_lock.sender().lock().await.send(Message::text(json.clone())).await.is_err() {
                    client_lock.set_status(ClientStatus::Closing);
                    println!("*** write error to client during wallop");    
                }
                count += 1;
            }
        }
       count 
    }

    pub async fn sendto_wall(&mut self, message: CMessage) -> u64 {
        let json = match serde_json::to_string(&message) {
            Ok(j)   => j,
            Err(e)  => { println!("*** failed sendto_wall(): unable to serialize CMessage [{}]", e); return 0 }
        };
        let mut count = 0u64;

        for client in self.clients.lock().await.values() {
            if client.read().await.status() as u64 == ClientStatus::Connected as u64 {
                let mut client_lock = client.write().await;
                count += 1;

                if client_lock.sender().lock().await.send(Message::text(json.clone())).await.is_err() {
                    client_lock.set_status(ClientStatus::Closing);
                    println!("*** write error to client during wall");    
                }
            }
        }
       count 
    }

    pub async fn sendto_one(&mut self, id: Uuid, message: CMessage) -> u64 {
        let json = match serde_json::to_string(&message) {
            Ok(j)   => j,
            Err(e)  => { println!("*** failed sendto_wall(): unable to serialize CMessage [{}]", e); return 0 }
        };

        for client in self.clients.lock().await.values() {
            if client.read().await.id() == id {
                let mut client_lock = client.write().await;

                if client_lock.sender().lock().await.send(Message::text(json.clone())).await.is_err() {
                    client_lock.set_status(ClientStatus::Closing);
                    println!("*** write error to client during wall");    
                }

                1;
            }
        }
        0
    }
    
    pub async fn get_channels(&self) -> HashMap<Uuid, Arc<RwLock<Channel>>> {
        let list = self.channels.lock().await.clone();
        list
    }
    pub async fn add_channel(&mut self, channel: Channel) {
        self.channels.lock().await.insert(channel.id(), Arc::new(RwLock::new(channel)));
    }

    pub async fn remove_channel(&mut self, id: Uuid) {
        self.channels.lock().await.remove(&id);
    }

    pub async fn get_channel_ref(&self, id: Uuid) -> Option<Arc<RwLock<Channel>>> {
        match self.channels.lock().await.get(&id) {
            None    => None,
            Some(chan)   => Some(chan.clone())
        }
    }
    pub async fn get_channel_by_name(&self, name: String) -> Option<Arc<RwLock<Channel>>> {
        for chan in self.channels.lock().await.values() {
            if chan.try_read().unwrap().name() == name {
                return Some(Arc::clone(chan))
            }
        } 
        None
    }
    
    pub async fn channel_exists(&self, id: Uuid) -> bool {
        self.channels.lock().await.get(&id).is_some()
    }

    pub async fn channel_name_to_uuid(&self, name: String) -> Option<Uuid> {
        for chan in self.channels.lock().await.values() {
            let lock = chan.read().await;
            if lock.name().to_lowercase() == name.to_lowercase() {
                return Some(lock.id())
            }
        }
        None
    }

    pub async fn channel_uuid_to_name(&self, id: Uuid) -> Option<String> {
        for (uid, chan) in self.channels.lock().await.iter() {

        }
        None
    }

    pub async fn channel_count(&self) -> u64 {
        self.channels.lock().await.len() as u64
    }

    pub async fn create_default_channels(&mut self, config: &Config) {
        let mut chan_creator: Channel;

        if config.use_global_lobby {
            chan_creator = Channel::new("Global Lobby".to_string(), None, false);
            chan_creator.set_options(
                ChannelOptions::Persist
                    .or(ChannelOptions::RejoinClients)
                    .or(ChannelOptions::SaveHistory)
                    .or(ChannelOptions::CanNotLeave)
                    // .or(ChannelOptions::Invisible)
            );
            chan_creator.set_topic(Some(format!("InvenT chatd v{} (unreleased) - imightanswer@clearchat.club", VERSION).to_string())).await;
            self.add_channel(chan_creator).await;
        }

        if config.use_staff_lobby {
            chan_creator = Channel::new("Staff Lobby".to_string(), None, false);
            chan_creator.set_options(
                ChannelOptions::Persist
                    .or(ChannelOptions::AgentOnly)
                    // .or(ChannelOptions::HiddenMemberList)
                    .or(ChannelOptions::RejoinClients)
                    .or(ChannelOptions::SaveHistory)
                    .or(ChannelOptions::CanNotLeave)
            );
            chan_creator.set_topic(Some("DO NOT GIVE OUT YOUR PASSWORDS".to_string())).await;
            self.add_channel(chan_creator).await;
        }

        if config.use_guest_lobby {
            chan_creator = Channel::new("Guest Lobby".to_string(), None, false);
            chan_creator.set_options(
                ChannelOptions::Persist
                    // .or(ChannelOptions::HiddenMemberList)
                    // .or(ChannelOptions::HiddenMessages)
                    .or(ChannelOptions::RejoinClients)
                    .or(ChannelOptions::CanNotLeave)
                    // .or(ChannelOptions::Invisible)
            );
            self.add_channel(chan_creator).await;
        }

    }
}
