use crate::network::tcp_server::TcpServer;
use common_log::log::info;
use common_version::banner;
use flume::{Receiver, Sender};
use std::{fmt::Result, net::SocketAddr, time::Duration};
use tokio::{io, time::error::Elapsed};

#[derive(Debug, thiserror::Error)]
#[error("Acceptor error")]
pub enum Error {
    #[error("I/O {0}")]
    Io(#[from] io::Error),
    #[error("Timeout")]
    Timeout(#[from] Elapsed),
}

pub struct Broker {
    accept_thread_num: usize,
    max_connection_num: usize,
    request_queue_size: usize,
    response_queue_size: usize,
    signal_st: Sender<u16>,
    signal_rt: Receiver<u16>,
}

impl Broker {
    pub fn new(
        accept_thread_num: usize,
        max_connection_num: usize,
        request_queue_size: usize,
        response_queue_size: usize,
    ) -> Broker {
        let (signal_st, signal_rt) = flume::bounded::<u16>(1);
        return Broker {
            accept_thread_num,
            max_connection_num,
            request_queue_size,
            response_queue_size,
            signal_st,
            signal_rt,
        };
    }
    pub async fn start(&self) -> Result {
        // metrics init

        // tcp server start
        let ip: SocketAddr = "127.0.0.1:8768".parse().unwrap();
        let net_s = TcpServer::new(
            ip,
            self.accept_thread_num,
            self.max_connection_num,
            1,
            1,
            1,
            1,
        );
        net_s.start().await;

        // grpc server start

        // http server start

        // process start hook
        banner();

        loop {
            if let Ok(sig) = self.signal_rt.recv() {
                if sig == 1 {
                    info("Start to stop network processes!");
                    break;
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await
        }

        return Ok(());
    }

    pub async fn stop(&self) -> Result {
        // Recovery of resources

        // Sends a signal to stop the process
        self.signal_st.send(1).unwrap();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::thread::sleep;
    use std::time::Duration;

    use super::Broker;
    use bytes::{BufMut, Bytes, BytesMut};
    use common_base::runtime::create_runtime;
    use futures::executor::block_on;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpStream,
    };

    #[test]
    fn start_broker() {
        let rt = create_runtime("text", 10);
        let guard = rt.enter();
        let b = Broker::new(10, 10, 0, 0);
        _ = block_on(b.start());
        drop(guard);
    }

    #[test]
    fn client() {
        let rt = create_runtime("text", 10);
        let guard = rt.enter();
        tokio::spawn(async move {
            let mut stream = TcpStream::connect("127.0.0.1:8768").await.unwrap();
            // let mut stream = Framed::new(stream, LengthDelimitedCodec::new());
            let mut write_buf = BytesMut::with_capacity(20);
            write_buf.put(&b"hello world lobo"[..]);
            let _ = stream.write_all(&write_buf).await;

            let mut read_buf = BytesMut::with_capacity(20);
            match stream.read_buf(&mut read_buf).await {
                Ok(_) => {
                    let content = String::from_utf8_lossy(&read_buf).to_string();
                    println!("receive:{}", content)
                }
                Err(err) => {
                    println!("err:{:?}", err)
                }
            }
        });
        drop(guard);
        sleep(Duration::from_secs(10));
    }
}