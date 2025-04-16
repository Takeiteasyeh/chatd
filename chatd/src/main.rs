mod config;
mod server;
mod client;
mod channel;
mod message;
mod commands;
mod auth;
mod ban;
use commands::CommandHandler;
use config::*;
use auth::*;
use futures_util::{SinkExt, StreamExt};
use rand::Rng;
use server::*;
use client::*;
use channel::*;
use message::{Message as CMessage,*};
use uuid::Uuid;
use ban::Ban;

// use tracing::{info, Level};
use tokio::net::TcpListener;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_rustls::{rustls::ServerConfig, TlsAcceptor};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use std::borrow::Borrow;
use std::fs::OpenOptions;
use std::io::Write;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use std::net::{IpAddr, Ipv4Addr};
use tokio::sync::{Mutex, mpsc};
// use sqlx::{sqlite::SqlitePoolOptions, Pool, Sqlite, query};

// use std::fs::File;
// use std::io::{self, BufReader};
const VERSION: &str = "0.14.0-beta";
const PING_TIMEOUT: u8 = 180;
const PING_CHECK_IDLE: u8 = 60;
const DEFAULT_CONFIG_FILE: &str = "chatd.conf";

#[tokio::main]
async fn main() {
    // console_subscriber::init();
    println!("**************************\n* InvenT chatd {} by Ray Lynk\n**************************", VERSION);
    let config = match Config::from_disk(DEFAULT_CONFIG_FILE) {
        Ok(config)  => { println!("ok: loaded config from {}", DEFAULT_CONFIG_FILE); config },
        Err(e)  => { 
            println!("unable to load config: {}  [{}]\ncreating new config", DEFAULT_CONFIG_FILE, e.to_string()); 
            let config = Config::new();
            if config.to_disk(DEFAULT_CONFIG_FILE).is_ok() {
                println!("created and saved new config to {}", DEFAULT_CONFIG_FILE);
                config
            } else {
                println!("unable save config to {}", DEFAULT_CONFIG_FILE);
                config
            }
        }
    };
   
    let certificates: Vec<_> = match CertificateDer::pem_file_iter(config.ssl_certificate.to_owned()) {
        Ok(certs)   => certs.map(|cert| cert.unwrap()).collect(),
        Err(e)      => {
            println!("fatal: unable to parse certificate file: {} [{}]", config.ssl_certificate.to_owned(), e);
            std::process::exit(0);
        }
    };

    let privatekey = match PrivateKeyDer::from_pem_file(config.ssl_privatekey.to_owned()) {
        Ok(key) => key,
        Err(e)  => {
            println!("fatal: unable to parse private key file: {} [{}]", config.ssl_privatekey.to_owned(), e);
            std::process::exit(0);
        }
    };

    let tlsconfig = match ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certificates, privatekey) {
        Ok(conf)    => conf,
        Err(e)      => {
            println!("fatal: unable to build serverconfig with ssl certificates [{}]", e);
            std::process::exit(0)
        }
    };

    let listener = match TcpListener::bind("0.0.0.0:1300").await {
        Ok(bind)    => bind,
        Err(e)      => {
            println!("fatal: unable to bind to {}:{} [{}]", config.bind_address, config.bind_port, e);
            std::process::exit(0);
        }
    };

    let tls_acceptor = TlsAcceptor::from(Arc::new(tlsconfig));

    println!("ok: bind - {}:{} ", config.bind_address.to_owned(), config.bind_port);
    // load our auth module
    // let mut authfinder: Arc<dyn AuthFinder + 'static + Send>;
    let authfinder: Arc<Mutex<Box<dyn AuthFinder + Send + Sync>>>;

    match config.to_owned().auth_type {
        AuthType::SqLite(path)  => {
            println!("auth-type: SQLite {}", path.clone());
            match AuthSqLite::new(path.clone(), config.auth_salt.to_owned()).await {
                Ok(auth)    => { authfinder = Arc::new(Mutex::new(Box::new(auth))) },
                Err(e)      => { println!("fatal: unable to open agent database at {}\nerror: {}", path, e); std::process::exit(0);}
            }
        },

        AuthType::Invent(_url, _apikey)   => {
            println!("fatal: AuthType::Invent is not available to this version. Please use AuthType::SqLite");
            std::process::exit(0);
            // println!("auth-type: Invent: {}", url.clone());
            // auth_sql = Box::new(AuthInvent { url, session: apikey });
        } 
    }

    authfinder.lock().await.create_tables().await;
    let has_any = authfinder.lock().await.has_any().await;

    match has_any { 
        Err(_)      => { println!("database: error checking for agents."); },
        Ok(true)    => { println!("database: found agents in the database."); () },
        Ok(false)   => {
            println!("database: no agents found in database, generating...");
            let random_username = "admin";
            let random_password = rand::thread_rng().gen_range(999999..99999999).to_string();
    

            // if authfinder.lock().await.create_tables().await < 0 {
            //     println!("database fatal error: unable to create database tables.");
            //     std::process::exit(1);
            // }

            match authfinder.lock().await.add(random_username, &random_password, ClientOptions::Admin).await {
                Ok(_) => { println!("database: created admin account:\nusername: {}\npassword: {}\n\n---------------------\n", random_username, random_password); },
                Err(e) => { println!("database: unable to create admin account: {}", e.to_string()); }
            }
        }
    } 
    let (server_tx, server_rx) = mpsc::unbounded_channel();
    let server = Arc::new(Mutex::new(Server::new(server_rx, config.clone())));

    ////////////// test
    //142.188.205.60
    // server.lock().await.add_ban(IpAddr::V4(Ipv4Addr::new(142,188,205,60)), "bad person 1", SystemTime::now(), "siamesetwins".into()).await; 
    // if server.lock().await.add_ban(IpAddr::V4(Ipv4Addr::new(192,0,0,1)), "bad person 2", SystemTime::now(), "siwins".into()).await.is_ok() {
    //     println!("added 192 to bans");
    // } 
    // server.lock().await.save_ban_db("bans.db".into()).await;

    // if server.lock().await.ban_exists(IpAddr::V4(Ipv4Addr::new(195,0,0,1))).await {
    //     println!("ban exists for 192");
    //     
    // }
    ///
    //
    println!("ok: mpsc - rx/tx unbounded");
    let receiver_handle = Arc::clone(&server);
    let config_mpsc = config.clone();

    // this processes pipe data from any of our internal websockets, to the main channel pipe
    // tokio::task::Builder::new().name("mpsc eater").spawn(async move {
    tokio::spawn(async move {
        let config = config_mpsc;
        loop {
            {
                let mut lock = match receiver_handle.try_lock() {
                    Ok(lock)    => lock,
                    Err(_)      => { tokio::time::sleep(std::time::Duration::from_millis(10)).await; continue; }
                };
                let cmessage = match lock.receiver.try_recv() {
                    Ok(m)   => m,
                    Err(_)  => {
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                        continue;
                    }
                };

                std::mem::drop(lock);
                let my_client = receiver_handle.lock().await.get_client_ref(cmessage.source).await;

                if my_client.is_none() {
                    println!(">>>>> serious error: command from a client that doesnt exist: {}.", cmessage.source);
                    continue;
                }

                handle_client_command(&receiver_handle.clone(), config.clone(), &my_client.unwrap(), authfinder.clone(), cmessage).await;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }
    });

    println!("ok: inter-channel communication reader");

    let health_server = server.clone();

    // background tasks spawn, mostly for checking timeouts, ect
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(50)).await;
            check_client_pings(&health_server).await;
        }
    });

    println!("create default channels:\n\t-> Global Lobby: {}\n\t-> Staff Lobby: {}\n\t-> Guest Lobby: {}", config.use_global_lobby, config.use_staff_lobby, config.use_guest_lobby);
    { server.lock().await.create_default_channels(&config).await; }

    println!("ok: channels - created default lobbys");
    println!("ok: ready for connections");
    println!("channels:  {}", server.lock().await.channel_count().await);
    println!("bans loaded: {}", server.lock().await.count_bans().await);

    // start accepting client connections
    while let Ok((stream, ip)) = listener.accept().await {
        // check the ban list
        let mut serv_lock = server.lock().await;

        if serv_lock.ban_exists(ip.ip()).await {
            serv_lock.banned_connects += 1;
            println!("Banned({}): connection from {} refused.", serv_lock.banned_connects, ip.ip());
            continue;
        }
        
        std::mem::drop(serv_lock);

        let tls_acceptor = tls_acceptor.clone();
        let server = server.clone();
        let server_tx = server_tx.clone();
        let invalid = server.lock().await.invalid_connects;
        let mydate = SystemTime::now();
        println!("ts:{} -- connection attempt - {}", mydate.duration_since(UNIX_EPOCH).unwrap().as_secs(), ip);
        let server_lock = match server.try_lock() {
            Ok(server_lock) => server_lock,
            Err(_)  => { println!("main thread block"); continue; }
        };
        let (test_channelcount, test_clientcount) = tokio::join!(server_lock.channel_count(), server_lock.client_count());

        drop(server_lock);
        println!("we have {} channels and {} clients", test_channelcount, test_clientcount);
        // server.lock().await.invalid_connects += 1;

        tokio::task::spawn(async move {
            let tls_stream = match tls_acceptor.accept(stream).await {
                Ok(tls) => { println!("tls ok: {}", ip); tls },
                Err(e)  => { 
                    println!("rejected[{}]: tls failed - {}", invalid+1, e); 
                    // server.lock().await.invalid_connects += 1;
                    return;
                }
            };
            let sock_stream = match accept_async(tls_stream).await {
                Ok(ss)  => { println!("accepted: {} - websocket proto", ip); ss }
                Err(e)  => {
                    println!("rejected[{}]: {} - {}", invalid+1, ip, e);
                    // server.lock().await.invalid_connects += 1;
                    return;
                }
            };

            let (ws_sender, mut ws_receiver) = sock_stream.split();
            let my_client = Client::new(ws_sender, server_tx, ip.ip(), None);
            let my_uuid = my_client.id();
            server.lock().await.add_client(my_client).await;

            // reinit our client with locks
            let my_client = match server.lock().await.get_client_ref(my_uuid).await {
                Some(mc)    => mc,
                None        => {
                    println!("error: unable to refind {}", ip.ip());
                    return;
                }
            };

              while let Some(message) = ws_receiver.next().await {
                let main_sender = my_client.read().await.main_sender.clone();
                // we can't trust the source field from the sender, so we will remake it (if needed)
                if message.is_err() {
                    my_client.write().await.set_status(ClientStatus::Closing);
                    handle_client_error(my_client.as_ref(), &server, "read error: connection reset by peer".to_string()).await;
                    return;
                }
                
                // for debug
                let unwrap_msg = message.unwrap();
                println!("> {}", unwrap_msg.clone());
                let cmessage = serde_json::from_str::<CMessage>(&unwrap_msg.to_string());

                if cmessage.is_err() {
                    my_client.write().await.set_status(ClientStatus::Closing);
                    handle_client_error(my_client.as_ref(), server.borrow(), "read error: connection closed".to_string()).await;
                    return;
                }

                let mut cmessage = cmessage.unwrap();
                cmessage.source = my_client.read().await.id();
                let _ = main_sender.send(cmessage);
            }
           
        });
    }

}

async fn handle_client_error(client: &RwLock<Client>, server: &Arc<Mutex<Server>>, reason: String) {
    let client_lock = client.write().await;
    let channels = client_lock.channels().await;
    let client_id = client_lock.id();
    let client_name = client_lock.name();
    let client_ip = client_lock.ip();
    std::mem::drop(client_lock);

    // zombies have already been cleaned up elsewhere
    for (uid, channel_name) in channels {
        let channel_ref = server.lock().await.get_channel_ref(uid).await;

        if channel_ref.is_some() {
            let channel_ref = channel_ref.unwrap();
            channel_ref.write().await.remove_member(client_id.clone()).await;
            let cmessage = CMessage::new(MessageType::Quit(reason.clone()), client_id.clone() , uid.clone(), Some(client_name.clone()));
            
            let is_hidden = channel_ref.read().await.has_option(ChannelOptions::HiddenMemberList);
            let is_invisible = channel_ref.read().await.has_option(ChannelOptions::Invisible);
            let is_persist = channel_ref.read().await.has_option(ChannelOptions::Persist);
 
            if is_hidden || is_invisible {
                let _ = channel_ref.write().await.sendto_agents(&cmessage).await;
            } else {
                let _ = channel_ref.write().await.sendto_all_butone(client_id, &cmessage).await;
            }

            if channel_ref.read().await.count_members().await == 0 && !is_persist {
                channel_ref.read().await.to_log(format!("{:?} / DESTROY CHANNEL: {} ({}@{}) [Client Error Disconnect]", std::time::SystemTime::now(), channel_name, client_name, client_ip)).await;
                server.lock().await.remove_channel(uid).await;
            }
        }
    }

    server.lock().await.remove_client(client_id).await;
}

async fn check_client_pings(health_server: &Arc<Mutex<Server>>) {
{
    let mut expired_clients: Vec<Uuid> = Vec::new();
    let clients_list = health_server.lock().await.get_clients().await;


    if clients_list.is_some() {
            let clients = clients_list.unwrap();
            // println!("got {} clients to ping", clients.lock().await.len());
            let too_long = SystemTime::now() - std::time::Duration::from_secs(PING_CHECK_IDLE as u64);
            let kick_long = SystemTime::now() - std::time::Duration::from_secs(PING_TIMEOUT as u64);

            for client in clients.lock().await.values() {
                let mut lock = client.write().await;
            
                if lock.last_action() < too_long {
                    if lock.last_ping_time() < kick_long {
                        let _ = lock.sender().lock().await.send(Message::text("Disconnect (Ping Timeout)")).await;
                        // println!("*** {}@{} has disconnected (Ping Timeout: {} seconds)", lock.name(), lock.ip(), PING_TIMEOUT);
                        let _ = lock.sender().lock().await.close().await;
                        expired_clients.push(lock.id());
                        lock.set_status(ClientStatus::Zombie); // everything cleared,
                    }
                    else if lock.last_ping_time() < too_long {
                        // send a ping and reset last action to give them one more
                        // cycle to reply
                        // lock.sender().lock().await.send(Message::text("PING")).await;
                        let id = lock.id();
                        let _ = lock.sender().lock().await.send(CMessage::new(MessageType::Ping(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or(Duration::new(0, 0)).as_secs()), Uuid::nil(), id, None).into()).await;
                        lock.update_last_action_time();
                    }
                }
            }
        }
    
    for expired in expired_clients {
        println!("purge: {}", expired);
        let client_refs = health_server.lock().await.get_client_ref(expired).await;

        match client_refs {
            Some(client_exp)    => {
                for (uid, channel_name) in client_exp.read().await.channels().await {
                    let channel = health_server.lock().await.get_channel_ref(uid).await;

                    if channel.is_some() {
                        let channel = channel.unwrap();
                    
                        let cmessage = CMessage::new(MessageType::Quit(format!("ping timeout: {} seconds", PING_TIMEOUT).to_string()), expired.clone() , uid.clone(), Some(client_exp.read().await.name()));
                        let _ = channel.write().await.sendto_all_butone(expired, &cmessage).await;

                        channel.write().await.remove_member(expired).await;

                        let is_persist = channel.read().await.has_option(ChannelOptions::Persist);
                        
                        if channel.read().await.count_members().await == 0 && !is_persist {
                            channel.read().await.to_log(format!("{:?} / DESTROY CHANNEL: {} ({}@{}) [Client Pingout]", std::time::SystemTime::now(), channel_name, client_exp.read().await.name(), client_exp.read().await.ip())).await;
                            health_server.lock().await.remove_channel(uid).await;
                        } else {
                            channel.read().await.to_log(format!("{:?} / PART CHANNEL: {} ({}@{}) [Client Pingout]", std::time::SystemTime::now(), channel_name, client_exp.read().await.name(), client_exp.read().await.ip())).await;
                        }
                    }
                }

                client_exp.write().await.clear_channels().await;
                let _ = client_exp.write().await.sender().lock().await.close().await;
                health_server.lock().await.remove_client(expired).await;
            }
            None    => ()
        }
        health_server.lock().await.remove_client(expired).await;
    }
    }
}

async fn handle_client_command(server: &Arc<Mutex<Server>>, config: Config, my_client: &Arc<RwLock<Client>>, authfinder: Arc<Mutex<Box<dyn AuthFinder + Send + Sync>>>, message: CMessage) {
    let mut c_lock = my_client.write().await;
    c_lock.update_last_action_time();
    c_lock.update_last_ping_time();
    let cmessage = message;

    // if we have no auth, we must have auth, otherwise, close with no fingerprint
    if c_lock.status() as u64 == ClientStatus::PendingAuth as u64 {
        // we try to do message parsing twice, once here so we can fail quietly, and
        // once later so we can fail loudly
       match cmessage.r#type {
            MessageType::AuthGuest(username) => {
                
                // server.lock().await.connections_since_start += 1;
                // server.lock().await.invalid_connects -= 1;

                if !config.allow_guests {
                    let _ = c_lock.sender().lock().await.send(CMessage::new_problem(ProblemCode::NotAvailable, None, "Not accepting unauthenticated users at this time.".to_string()).into()).await;
                    let _ = c_lock.sender().lock().await.close().await;
                    return;
                }

                let actual_username: String;

                if username.is_some() {
                    actual_username = format!("Guest-{}", username.unwrap());
                } else {
                    actual_username = Client::generate_guest_name(None);
                }

                std::mem::drop(c_lock);
                let slock = server.lock().await;
                std::mem::drop(slock);

                if server.lock().await.client_name_to_uuid(&actual_username).await.is_some() {
                    let _ = my_client.write().await.sender().lock().await.send(CMessage::new_problem(ProblemCode::NameInUse, None, actual_username).into()).await;
                    return;
                }

                c_lock = my_client.write().await;
                
                if !c_lock.set_name(&actual_username).await {
                    let _ = c_lock.sender().lock().await.send(CMessage::new_problem(ProblemCode::NameInvalid, None, actual_username).into()).await;
                    return;
                }

                c_lock.set_status(ClientStatus::Connected);
                c_lock.set_type(ClientType::Guest);
                c_lock.set_options_u64(config.default_guest_options);
                server.lock().await.guest_count += 1;
                let id = c_lock.id();
                let name = c_lock.name();
                let _ = c_lock.sender().lock().await.send(CMessage::new(MessageType::AuthOk(id.to_owned(), name.to_owned()), Uuid::nil(), Uuid::nil(), None ).into()).await;
                let motd = server.lock().await.motd_guests.clone();
                
                if motd.len() > 0 {
                    let _ = c_lock.sender().lock().await.send(CMessage::new(MessageType::Motd(motd), Uuid::nil(), id, Some("Guest Message of the Day".to_string())).into()).await;
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

                if config.use_guest_lobby {
                    if let Some(global_channel) = server.lock().await.get_channel_by_name("Guest Lobby".to_string()).await {
                            let mut lock = global_channel.write().await;
                            println!("*** adding client {} to Guest Lobby", 1+lock.count_members().await);
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

                return;
            },
            MessageType::AuthDevice(deviceid, username) => {

            },
            MessageType::AuthAgent(nickname, username, password) => {
                std::mem::drop(c_lock);
                CommandHandler::auth_agent(server.as_ref(), my_client.as_ref(), authfinder.clone(), config.clone(), nickname, username, password).await;
                return;
            },
            _   => {
                println!("unsure what is");
                // bad request, we only allow auth at this stage.
                c_lock.set_status(ClientStatus::Closing);
                std::mem::drop(c_lock);
                handle_client_error(my_client.as_ref(), server, "bad request pre-auth".to_string()).await;
                return;
            }

        }
        return;
    } // end of authing

     // DROP LOCKS -- THIS MATCH CASE WILL BE HANDLED BY ANOTHER FUNCTION
    std::mem::drop(c_lock);

    match cmessage.r#type {
        MessageType::SetChannelModes(modes) => {
            CommandHandler::set_channel_modes(&server, &my_client, cmessage.target, modes).await;
            return;
        },
        MessageType::Topic(topic) => {
            CommandHandler::set_channel_topic(&server, &my_client, cmessage.target, topic, config.max_topic_length).await;
            return;
        },
        MessageType::Channels => {
            CommandHandler::channel_list(&server, &my_client).await;
            return;
        },
        MessageType::Join(channel, _) => {
            if channel.len() == 0 {
                CommandHandler::join(&server, &my_client, cmessage.target.to_string(), true).await;
            } else {
                CommandHandler::join(&server, &my_client, channel, false).await;
            }
            return;
        },
        MessageType::Part(_, channel, _) => {
            CommandHandler::part(&server, &my_client, channel).await;
            return;
        },
        MessageType::Kick(channel, userid, reason) => {
            CommandHandler::kick(&server, &my_client, channel, userid, reason).await;
            return;
        },
        MessageType::Kill(userid, reason) => {
            CommandHandler::kill(&server, &my_client, Uuid::from_str(&userid).unwrap_or(Uuid::nil()), reason).await;
            return;
        },
        MessageType::Kline(ip, expires_sec, reason) => {
            CommandHandler::kline(&server, &my_client, IpAddr::V4(Ipv4Addr::from_str(&ip).unwrap_or(Ipv4Addr::new(0, 0, 0, 0))), reason, expires_sec).await;
            return;
        },
        MessageType::Pong(_reply) => {
            println!("PONG: {}", my_client.as_ref().read().await.name());
            // we technically dont need to do anything, and dont care at this point.
        },

        MessageType::Typing(_, target) => {
            CommandHandler::typing(&server, &my_client, target).await;
            return;
        },

        MessageType::Message(message) => {
            let c_lock = my_client.read().await;
            let clean_message = CMessage::sanitize_text_message(message.clone());
            if let Some(chanref) = server.lock().await.get_channel_ref(cmessage.target).await {
                //ensure user is member of the channel
                if chanref.read().await.is_member(c_lock.id()).await {
                    if chanref.read().await.has_option(ChannelOptions::HiddenMessages) {
                        let _ = chanref.write().await.sendto_agents(&CMessage::new(MessageType::Message(clean_message.to_owned()), c_lock.id(), cmessage.target, Some(c_lock.name()))).await;
                    } else {
                        let _ = chanref.write().await.sendto_all_butone(c_lock.id(), &CMessage::new(MessageType::Message(clean_message.to_owned()), c_lock.id(), cmessage.target, Some(c_lock.name()))).await;
                    }
                    if chanref.read().await.has_option(ChannelOptions::SaveHistory) {
                        let logfile = OpenOptions::new()
                            .append(true)
                            .create(true)
                            .open(format!("logs/{}.log",chanref.read().await.id()));

                        if logfile.is_ok() {
                            println!("logfile is ok");
                            let log_entry = format!("{:?} / {} ({}): {}\n", std::time::SystemTime::now(), c_lock.ip(), c_lock.name(), clean_message);

                            let _ = logfile.unwrap().write(log_entry.as_bytes());
                        }
                    }
                }
            }    
        },
        
        _   => {
            println!("unknown command from client");
            return;
        }
    }
}

