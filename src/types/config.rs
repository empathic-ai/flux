#[cfg(feature = "bevy")]
use bevy::prelude::*;
use bevy_reflect::*;

const LOCALHOST: &str = "localhost";

#[cfg_attr(feature = "bevy", derive(Resource))]
#[derive(Clone)]
pub struct FluxConfig {
    host_name: String,
    server_port: Option<String>,
    is_localhost: bool
}

impl FluxConfig {
    pub fn new(host_name: String, server_port: Option<String>, is_localhost: bool) -> Self {
        Self {
            host_name,
            server_port,
            is_localhost
        }
    }

    pub fn get_hostname(&self) -> String {
        self.host_name.clone()
    }

    pub fn get_server_port(&self) -> Option<String> {
        self.server_port.clone()
    }

    pub fn get_site_hostname(&self) -> String {
        Self::get_site_hostname_from(self.get_hostname())
    }

    pub fn get_api_hostname(&self) -> String {
        Self::get_api_hostname_from(self.get_hostname())
    }

    pub fn get_localhost_api_hostname(&self) -> String {
        Self::get_api_hostname_from(LOCALHOST.to_string())
    }

    pub fn get_client_api_url(&self) -> String {
        if self.is_localhost {
            self.get_localhost_api_url()
        } else {
            self.get_api_url()
        }
    }

    pub fn get_client_site_url(&self) -> String {
        if self.is_localhost {
            self.get_localhost_site_url()
        } else {
            self.get_site_url()
        }
    }

    pub fn get_api_url(&self) -> String {
        format!("https://{}", self.get_api_hostname())
    }

    pub fn get_site_url(&self) -> String {
        format!("https://{}", self.get_site_hostname())
    }

    pub fn get_localhost_api_url(&self) -> String {
        if let Some(server_port) = self.get_server_port() {
            format!("http://{}:{}", self.get_localhost_api_hostname(), server_port)
        } else {
            format!("http://{}", self.get_localhost_api_hostname())
        }
    }

    pub fn get_localhost_site_url(&self) -> String {
        if let Some(server_port) = self.get_server_port() {
            format!("http://{}:{}", LOCALHOST.to_string(), server_port)
        } else {
            format!("http://{}", LOCALHOST.to_string())
        }
    }

    fn get_site_hostname_from(hostname: String) -> String {
        #[cfg(feature = "production")]
        return hostname;
        #[cfg(not(feature = "production"))]
        return format!("dev.{}", hostname);
    }

    fn get_api_hostname_from(hostname: String) -> String {
        #[cfg(feature = "production")]
        return format!("api.{}", hostname);
        #[cfg(not(feature = "production"))]
        return format!("dev-api.{}", hostname);
    }
}


