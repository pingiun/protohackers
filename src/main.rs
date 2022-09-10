use std::{io, net, str::FromStr};

use actix::{prelude::*, spawn};
use bytes::BytesMut;
use tokio::{net::{TcpListener, TcpStream}, io::{split, WriteHalf}};
use tokio_util::codec::{FramedRead, BytesCodec};


struct EchoSession {
    framed: actix::io::FramedWrite<BytesMut, WriteHalf<TcpStream>, BytesCodec>,
}

impl Actor for EchoSession {
    type Context = Context<Self>;

    fn stopping(&mut self, ctx: &mut Self::Context) -> Running {
        println!("Connection stopping...");
        Running::Stop
    }

    fn stopped(&mut self, ctx: &mut Self::Context) {
        println!("Connection stopped...");
    }

}

impl StreamHandler<Result<BytesMut, io::Error>> for EchoSession {
    fn finished(&mut self, ctx: &mut Self::Context) {
        self.framed.close();    
    }

    fn handle(&mut self, item: Result<BytesMut, io::Error>, ctx: &mut Self::Context) {
        match item {
            Ok(bytes) => {
                println!("Received bytes: {:?}", bytes);
                self.framed.write(bytes);
            },
            Err(e) => match e.kind() {
                io::ErrorKind::UnexpectedEof => {
                    println!("EOF!");
                    ctx.stop();
                },
                _ => {
                    println!("Error: {}", e);
                    ctx.stop();
                },
            }
        }
    }
}

impl actix::io::WriteHandler<io::Error> for EchoSession {}

impl EchoSession {
    fn new(framed: actix::io::FramedWrite<BytesMut, WriteHalf<TcpStream>, BytesCodec>) -> EchoSession {
        EchoSession { framed }
    }
}

pub fn tcp_server(s: &str) {
    // Create server listener
    let addr = net::SocketAddr::from_str(s).unwrap();

    spawn(async move {
        let listener = TcpListener::bind(&addr).await.unwrap();

        while let Ok((stream, addr)) = listener.accept().await {
            println!("Connection from: {}", addr);
            // let server = server.clone();
            EchoSession::create(|ctx| {
                let (r, w) = split(stream);
                EchoSession::add_stream(FramedRead::new(r, BytesCodec::new()), ctx);
                EchoSession::new(actix::io::FramedWrite::new(w, BytesCodec::new(), ctx))
            });
        }
    });
}


fn main() {
    let system = actix::System::new();

    let _addr = system.block_on(async {
        tcp_server("0.0.0.0:12345")
    });

    system.run().unwrap();
}
