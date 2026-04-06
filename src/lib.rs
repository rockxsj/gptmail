pub mod config;
pub mod db;
pub mod error;
pub mod models;
pub mod service;
pub mod smtp_server;
pub mod web;

pub use config::AppConfig;
pub use service::MailService;
