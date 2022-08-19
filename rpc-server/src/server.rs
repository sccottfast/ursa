use anyhow::Result;
use axum::{Extension, Router};
use ipld_blockstore::BlockStore;
use std::{net::SocketAddr, sync::Arc};

use crate::{
    api::NodeNetworkInterface,
    config::ServerConfig,
    http,
    rpc::{self, rpc::RpcServer},
    service::MultiplexService,
};
use tracing::info;

pub struct Server<S>
where
    S: BlockStore + Sync + Send + 'static,
{
    rpc_server: RpcServer,
    interface: Arc<NodeNetworkInterface<S>>,
}

impl<S> Server<S>
where
    S: BlockStore + Sync + Send + 'static,
{
    pub fn new(config: &ServerConfig, interface: Arc<NodeNetworkInterface<S>>) -> Self {
        Self {
            rpc_server: RpcServer::new(&config, Arc::clone(&interface)),
            interface: interface.clone(),
        }
    }

    pub async fn start(&self, config: ServerConfig) -> Result<()> {
        info!("Server (Rpc and http) starting up");
        let rpc_router = Router::new()
            .merge(rpc::routes::network::init())
            .layer(Extension(self.rpc_server.clone()));

        let http = Router::new()
            .merge(http::routes::network::init::<S>())
            .layer(Extension(self.interface.clone()));

        let http_address = SocketAddr::from(([0, 0, 0, 0], config.port));

        let service = MultiplexService::new(http, rpc_router);

        info!("listening on {}", http_address);
        axum::Server::bind(&http_address)
            .serve(tower::make::Shared::new(service))
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use db::{rocks::RocksDb, rocks_config::RocksDbConfig};
    use libp2p::{identity::Keypair, PeerId};
    use simple_logger::SimpleLogger;
    use store::Store;
    use tracing::log::LevelFilter;

    use network::{config::UrsaConfig, service::UrsaService};

    fn ursa_network_init(
        config: &UrsaConfig,
        store: Arc<Store<RocksDb>>,
    ) -> (UrsaService<RocksDb>, PeerId) {
        let keypair = Keypair::generate_ed25519();
        let local_peer_id = PeerId::from(keypair.public());

        let service = UrsaService::new(keypair, config, store);

        (service, local_peer_id)
    }

    #[tokio::test]
    async fn test_rpc_start() {
        SimpleLogger::new()
            .with_utc_timestamps()
            .with_level(LevelFilter::Info)
            .with_colors(true)
            .init()
            .unwrap();

        let config = ServerConfig {
            port: 4069,
            addr: "0.0.0.0".to_string(),
        };

        let db = RocksDb::open("test_db", &RocksDbConfig::default())
            .expect("Opening RocksDB must succeed");
        let db = Arc::new(db);
        let store = Arc::new(Store::new(Arc::clone(&db)));

        let ursa_config = UrsaConfig::default();
        let (ursa_node, _) = ursa_network_init(&ursa_config, Arc::clone(&store));
        let ursa_node_sender = ursa_node.command_sender().clone();

        let interface = Arc::new(NodeNetworkInterface {
            store,
            network_send: ursa_node_sender,
        });

        let rpc = Server::new(&config, interface);

        let _ = rpc.start(config).await;
    }
}
