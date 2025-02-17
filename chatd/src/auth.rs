// use std::collections::HashMap;
// use crate::{Sqlite, SqlitePoolOptions, query, Pool};
use sha2::{Sha256, Digest};
// use std::sync::Arc;
// use tokio::sync::Mutex;
use crate::ClientOptions;
use async_trait::async_trait;
use sqlx::{sqlite::SqlitePoolOptions, Pool, Sqlite, Row, Error as SQLError};

#[derive(Clone)]
pub struct AuthSqLite {
    file: String,
    connection: Pool<Sqlite>,
    salt: String
}

pub struct AuthInvent {
    pub url: String,
    pub session: String
}

#[derive(Clone)]
pub struct UserAuthEntry {
    // id: u32,
    pub username: String,
    // password: String, // this is actually a hash
    pub permissions: u64,
}

impl AuthSqLite {
    pub async fn new(file: String, salt: String) -> Result<Self, String> {
        match SqlitePoolOptions::new().max_connections(5).connect(&file).await {
            Ok(connection) => { return Ok(AuthSqLite { file, connection, salt }) },
            Err(e) => return Err(e.to_string())
        };
    }

}
#[async_trait]
impl AuthFinder for AuthInvent {
    async fn by_username_password(&self, _username: &str, _password: &str) -> Option<UserAuthEntry> {
        unimplemented!("Not for this version");
    }
    async fn by_username(&self, _username: &str) -> Option<UserAuthEntry> {
        unimplemented!("Not for this version");
    }
    fn all(&self) -> Option<Vec<UserAuthEntry>> {
        unimplemented!("Not for this version");
    }
    async fn add(&mut self, _username: &str, _password: &str, _permissions: ClientOptions) -> Result<UserAuthEntry, String> {
        unimplemented!("Not for this version");
    }
    fn delete(&mut self, _username: &str) -> Result<(), String> {
        unimplemented!("Not for this version");
    }
    fn hash_password(&self, _password: &str, _salt: &str) -> String {
        unimplemented!("Not for this version");
    }
    async fn has_any(&self) -> Result<bool, ()> {
        unimplemented!("Not for this version");
    }

    async fn create_tables(&mut self) -> i8 {
        unimplemented!("Not for this version");
    }
}

#[async_trait]
impl AuthFinder for AuthSqLite {
    /// Searches for a record using a username and password.
    ///
    /// This is generally used to look up a record during auth
    async fn by_username_password(&self, username: &str, password: &str) -> Option<UserAuthEntry> {
        let query = sqlx::query("SELECT `permissions` FROM `agents` WHERE `username` = ? AND `password` = ? LIMIT 1")
            .bind(username)
            .bind(self.hash_password(password, &self.salt))
            .fetch_one(&self.connection)
            .await;

        if query.is_err() { return None; }
        
        Some(UserAuthEntry { username: username.to_string(), permissions: query.unwrap().get(0) }) 
    }
    
    /// Searches for a record using just a username
    async fn by_username(&self, username: &str) -> Option<UserAuthEntry> {
        let query = sqlx::query("SELECT `permissions` FROM `agents` WHERE `username` = ? LIMIT 1")
            .bind(username)
            .fetch_one(&self.connection)
            .await;

        if query.is_err() { 
            match query {
                Ok(_) => { return None; }
                Err(SQLError::RowNotFound) => { return None; }
                Err(e)  => { println!("other error {}", e.to_string()); return None; }
            }
            // return None; 
        }
        
        Some(UserAuthEntry { username: username.to_string(), permissions: query.unwrap().get(0) })
    }

    fn all(&self) -> Option<Vec<UserAuthEntry>> {

        None
    }

    /// Adds a username and password to the database.
    ///
    /// The password will be encrypted automatically when passed
    async fn add(&mut self, username: &str, password: &str, permissions: ClientOptions) -> Result<UserAuthEntry, String> {
        if self.by_username(username).await.is_some() {
            return Err("already exists".to_string());
        }

        let hash = self.hash_password(password, &self.salt);
        // let permission_int = permissions.
        let query = sqlx::query("INSERT INTO `agents` (`username`, `password`, `permissions`) VALUES (?, ?, ?)")
            .bind(username)
            .bind(hash)
            .bind(permissions.bits() as i64)
            .execute(&self.connection)
            .await;

        if query.is_err() { 
            return Err(query.unwrap_err().to_string()); }

        Ok(UserAuthEntry { username: username.to_string(), permissions: permissions.bits() })

    }

    fn delete(&mut self, username: &str) -> Result<(), String> {

        Ok(())
    }

    /// Checks if the database has any agents, returns result ok or false or err if a problem.
    async fn has_any(&self) -> Result<bool, ()> {
        let query = sqlx::query("SELECT `id` FROM `agents` LIMIT 1")
            .fetch_one(&self.connection)
            .await;
        
        match query {
            Ok(_)   => return Ok(true),
            Err(sqlx::Error::RowNotFound) => return Ok(false),
            Err(e) => { println!("error: {}", e.to_string()); return Err(()) }
        }
    }

    fn hash_password(&self, password: &str, salt: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(format!("{}+{}", password, salt));
        hasher.finalize().iter().map(|byte| format!("{:02x}", byte)).collect()
    }
    async fn create_tables(&mut self) -> i8 {
        let query = sqlx::query("SELECT `id` FROM `agents` LIMIT 1")
            .fetch_one(&self.connection)
            .await;

        match query {
            Ok(_)   => return 018,
            Err(_) => {} // not found, try and create it next       
        }

        match sqlx::query("CREATE TABLE `agents` (id INTEGER PRIMARY KEY, username TEXT NOT NULL UNIQUE, password BLOB NOT NULL, permissions INTEGER NOT NULL DEFAULT 0)")
            .execute(&self.connection)
            .await {
            Ok(_) => return 1i8,
            Err(_)=> return -1i8
        }
    }
}

#[async_trait]
pub trait AuthFinder: Send + Sync { 
    async fn by_username_password(&self, username: &str, password: &str) -> Option<UserAuthEntry>;
    async fn by_username(&self, username: &str) -> Option<UserAuthEntry>;
    fn all(&self) -> Option<Vec<UserAuthEntry>>;
    async fn add(&mut self, username: &str, password: &str, permissions: ClientOptions) -> Result<UserAuthEntry, String>;
    fn delete(&mut self, username: &str) -> Result<(), String>;
    fn hash_password(&self, password: &str, salt: &str) -> String;
    async fn has_any(&self) -> Result<bool,()>;
    async fn create_tables(&mut self) -> i8;
}
