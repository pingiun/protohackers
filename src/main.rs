use std::{collections::HashMap, net, str::FromStr, sync::Arc};

use actix::{prelude::*, spawn};
use bstr::ByteSlice;
use tokio::net::UdpSocket;

#[derive(Debug, Default)]
struct DataServer {
    data: HashMap<Vec<u8>, Vec<u8>>,
}

#[derive(Debug, Message)]
#[rtype(result = "()")]
struct SetData {
    key: Vec<u8>,
    value: Vec<u8>,
}

#[derive(Debug, Message)]
#[rtype(result = "Option<Vec<u8>>")]
struct GetData {
    key: Vec<u8>,
}

impl Actor for DataServer {
    type Context = Context<Self>;
}

impl Handler<SetData> for DataServer {
    type Result = ();

    fn handle(&mut self, msg: SetData, _ctx: &mut Self::Context) -> Self::Result {
        self.data.insert(msg.key, msg.value);
    }
}

impl Handler<GetData> for DataServer {
    type Result = Option<Vec<u8>>;

    fn handle(&mut self, msg: GetData, _ctx: &mut Self::Context) -> Self::Result {
        self.data.get(&msg.key).map(|x| x.clone())
    }
}

async fn handle_message(sock: Arc<UdpSocket>, addr: net::SocketAddr, dataserver: Addr<DataServer>, msg: Vec<u8>) {
    let split: Vec<&[u8]> = msg.splitn_str(2, b"=").collect();
    if split.len() == 1 {
        if msg == b"version" {
            sock.send_to(b"version=Jelle's key-value dinges 0.1", addr).await.unwrap();
            return;
        }
        // No = found, this is a retrieve request
        let data = dataserver.send(GetData{key: msg.clone()}).await;
        match data {
            Ok(Some(value)) => {
                let mut buf = Vec::new();
                buf.extend_from_slice(&msg);
                buf.extend_from_slice(b"=");
                buf.extend_from_slice(&value);
                sock.send_to(&buf, addr).await.unwrap();
            },
            Ok(None) => {
                let mut buf = Vec::new();
                buf.extend_from_slice(&msg);
                buf.extend_from_slice(b"=");
                sock.send_to(&buf, addr).await.unwrap();
            },
            Err(e) => {
                eprintln!("Error retrieving data: {}", e);
            },
        }
    } else {
        // Insertion request
        dataserver.send(SetData {key: split[0].to_vec(), value: split[1].to_vec()}).await.unwrap();
    }
}

fn udp_server(s: &str) {
    // Create server listener
    let addr = net::SocketAddr::from_str(s).unwrap();

    spawn(async move {
        let sock = UdpSocket::bind(&addr).await.unwrap();
        let server = DataServer::default().start();
        let s = Arc::new(sock);
        let mut buf = [0; 1024];
        loop {
            let (len, addr) = s.recv_from(&mut buf).await.unwrap();
            println!("{:?} bytes received from {:?}", len, addr);
            println!("{:?}", &buf[..len]);
            let message = Vec::from(&buf[..len]);
            let sender = Arc::clone(&s);
            let data = server.clone();
            spawn(async move {
                handle_message(sender, addr, data, message).await;
            });
        }
    });
}

fn main() {
    let system = actix::System::new();

    system.block_on(async { udp_server("0.0.0.0:12345") });

    system.run().unwrap();
}
