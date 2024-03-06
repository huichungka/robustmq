use std::sync::Arc;

use clients::ClientPool;
use cluster::{register_storage_engine_node, report_heartbeat, unregister_storage_engine_node};
use common::{
    config::storage_engine::StorageEngineConfig, log::info_meta,
    metrics::register_prometheus_export, runtime::create_runtime,
};
use protocol::storage_engine::storage::storage_engine_service_server::StorageEngineServiceServer;
use services::StorageService;
use tokio::{
    runtime::Runtime,
    signal,
    sync::{broadcast, Mutex},
};
use tonic::transport::Server;

mod cluster;
mod index;
mod metadata;
mod raft_group;
mod record;
mod segment;
mod services;
mod shard;
mod storage;
mod v1;
mod v2;

pub struct StorageEngine {
    config: StorageEngineConfig,
    stop_send: broadcast::Sender<bool>,
    server_runtime: Runtime,
    daemon_runtime: Runtime,
    client_poll: Arc<Mutex<ClientPool>>,
}

impl StorageEngine {
    pub fn new(config: StorageEngineConfig, stop_send: broadcast::Sender<bool>) -> Self {
        let server_runtime =
            create_runtime("storage-engine-server-runtime", config.runtime_work_threads);

        let daemon_runtime = create_runtime("daemon-runtime", config.runtime_work_threads);

        let client_poll: Arc<Mutex<ClientPool>> = Arc::new(Mutex::new(ClientPool::new()));

        return StorageEngine {
            config,
            stop_send,
            server_runtime,
            daemon_runtime,
            client_poll,
        };
    }

    pub fn start(&self) {
        // Register Node
        self.register_node();

        // start GRPC && HTTP Server
        self.start_server();

        // Threads that run the daemon thread
        self.start_daemon_thread();

        self.waiting_stop();
    }

    // start GRPC && HTTP Server
    fn start_server(&self) {
        // start grpc server
        let port = self.config.grpc_port;
        self.server_runtime.spawn(async move {
            let ip = format!("0.0.0.0:{}", port).parse().unwrap();
            info_meta(&format!(
                "RobustMQ StorageEngine Grpc Server start success. bind port:{}",
                ip
            ));

            let service_handler = StorageService::new();

            Server::builder()
                .add_service(StorageEngineServiceServer::new(service_handler))
                .serve(ip)
                .await
                .unwrap();
        });

        // start prometheus http server
        let prometheus_port = self.config.prometheus_port;
        self.server_runtime.spawn(async move {
            register_prometheus_export(prometheus_port).await;
        });
    }

    // Start Daemon Thread
    fn start_daemon_thread(&self) {
        let config = self.config.clone();
        let client_poll = self.client_poll.clone();
        self.daemon_runtime
            .spawn(async move { report_heartbeat(client_poll, config) });
    }

    // Wait for the service process to stop
    fn waiting_stop(&self) {
        self.daemon_runtime.block_on(async move {
            loop {
                signal::ctrl_c().await.expect("failed to listen for event");
                match self.stop_send.send(true) {
                    Ok(_) => {
                        info_meta("When ctrl + c is received, the service starts to stop");
                        self.stop_server().await;
                        break;
                    }
                    Err(_) => {}
                }
            }
        });
    }

    fn register_node(&self) {
        self.daemon_runtime.block_on(async move {
            register_storage_engine_node(self.client_poll.clone(), self.config.clone()).await;
        });
    }
    async fn stop_server(&self) {
        // unregister node
        unregister_storage_engine_node(self.client_poll.clone(), self.config.clone()).await;
    }
}
