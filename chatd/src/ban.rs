use std::fs;
use std::time::Duration;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::Deserialize;
use serde::Serialize;

use crate::{IpAddr, Ipv4Addr};
use crate::SystemTime;

#[derive(Serialize, Deserialize, Clone)]
pub struct Ban {
    pub ip: IpAddr,
    pub reason: String,
    pub added_by: String,
    pub added_on: SystemTime,
    pub expires: SystemTime
}
impl Default for Ban {
    fn default() -> Ban {
        Ban {
            ip: IpAddr::V4(Ipv4Addr::new(0,0,0,0)),
            reason: "No reason was provided".to_string(),
            added_by: "Server".to_string(),
            added_on: SystemTime::now(),
            expires: SystemTime::now().checked_add(Duration::new(3600, 0)).unwrap()
        }
    }
}

impl Ban {
    pub fn new(ip: IpAddr, reason: String, expires: SystemTime, added_by: String) -> Self {
        Ban {
            ip, reason, expires, added_by, ..Ban::default()
        }
    }

    pub async fn exists(bans: &Arc<RwLock<Vec<Ban>>>, ip: IpAddr) -> bool {

        bans.read().await.iter().any(|ban| ban.ip == ip)
    }

    pub fn load_bans(file: &str) -> Result<Vec<Ban>, std::io::Error> {
        let bans_raw = match fs::read(file) {
            Ok(contents) => contents,
            Err(_)  => {
                match fs::write(file, b"") {
                    Ok(()) => Vec::new(),
                    Err(e) => return Err(e)
                }
            }

        };

        if bans_raw.len() == 0 {
            return Ok(Vec::new());
        }

        Ok(serde_json::from_slice(&bans_raw).unwrap_or(Vec::new()))

    }

    pub fn save_to_disk(file: &str, bans: &Vec<Ban>) -> Result<(), String> {
        // if there are no bans we can bail early
        if bans.len() == 0 {
            return Ok(());
        }

        let serialized = match serde_json::to_vec(&bans) {
            Ok(encoded) => encoded,
            Err(e)  => return Err(e.to_string())
        };

        match fs::write(file, serialized) {
            Ok(()) => return Ok(()),
            Err(e) => return Err(format!("unable to write to file '{}': {}", file, e.to_string()).into())
        }
    }
}
