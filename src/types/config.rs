use bevy::prelude::*;

#[derive(Resource, Clone)]
pub struct FluxConfig {
    host_name: String,
    server_port: String
}

impl FluxConfig {
    pub fn new(host_name: String, server_port: String) -> Self {
        Self {
            host_name,
            server_port
        }
    }

    pub fn get_hostname(&self) -> String {
        self.host_name.clone()
    }

    pub fn get_site_hostname(&self) -> String {
        #[cfg(feature = "production")]
        return self.get_hostname();
        #[cfg(not(feature = "production"))]
        return format!("dev.{}", self.get_hostname());
    }

    pub fn get_server_port(&self) -> String {
        self.server_port.clone()
    }

    pub fn get_api_hostname(&self) -> String {
        #[cfg(feature = "production")]
        return format!("api.{}", self.get_hostname());
        #[cfg(not(feature = "production"))]
        return format!("dev-api.{}", self.get_hostname());
    }

    pub fn get_site_url(&self) -> String {
        format!("https://{}:{}", self.get_site_hostname(), self.get_server_port())
    }

    pub fn get_api_url(&self) -> String {
        format!("https://{}:{}", self.get_api_hostname(), self.get_server_port())
    }

    pub fn get_api_origin(&self) -> String {
        format!("https://{}", self.get_api_hostname())
    }
}


