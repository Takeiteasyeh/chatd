use std::{fs, net::{IpAddr, Ipv4Addr}};
use std::error::Error;

use serde::{Serialize, Deserialize};

#[derive(Clone, Serialize, Deserialize)]
pub enum AuthType {
    Invent(String, String), // String,String is a url to the invent auth instance and api key
    SqLite(String)    // String is a path to a .auth file that will be created.
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    pub bind_address: IpAddr,
    pub ssl_certificate: String, // path to ssl certificate
    pub ssl_privatekey: String, // path to ssl private key
    pub bind_port: u16,
    pub auth_type: AuthType,
    pub auth_salt: String,
    pub allow_clients: bool, // allow customer access
    pub allow_guests: bool,  // allow guests (ie pub website)
    pub default_guest_options: u64,
    pub default_agent_options: u64,
    pub ban_db: String, // file to store bans
    pub motd_file_guests: String,
    pub motd_file_clients: String,
    pub motd_file_agents: String,
    pub use_global_lobby: bool, // use global lobby for all connections
    pub use_staff_lobby: bool,  // use global lobby for all agents
    pub use_guest_lobby: bool,  // use global lobby for all guests 
    pub max_topic_length: u16,
    
}
impl Config {
    pub fn new() -> Self {
        Config { ..Default::default()}
    }

    pub fn from_disk(filename: &str) -> Result<Config, Box<dyn Error>> {
        let config: Config = serde_json::from_str(&fs::read_to_string(filename)?)?;
        Ok(config)
    }

    pub fn to_disk(&self, filename: &str) -> Result<(), Box<dyn Error>> {
        fs::write(filename, &serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            bind_address: IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
            bind_port: 1300,
            ssl_certificate: "/path/to/ssl_certs/cert.pem".to_string(),
            ssl_privatekey: "/path/to/ssl_certs/privkey.pem".to_string(),
            auth_type: AuthType::SqLite("chatd.auth".to_string()),
            auth_salt: "5805fde87d8cbf0de22c396419f10bfa652f3e95225b50f1d446e1b225db4745".to_string(),
            allow_clients: true,
            allow_guests: true,
            default_guest_options: 12u64, // 12u64
            default_agent_options: 62u64,
            ban_db: "bans.db".to_string(),
            motd_file_guests: "guest.motd".to_string(),
            motd_file_clients: "client.motd".to_string(),
            motd_file_agents: "agent.motd".to_string(),
            use_staff_lobby: true,
            use_guest_lobby: true,
            use_global_lobby: true,
            max_topic_length: 128,
        }
    }
}
