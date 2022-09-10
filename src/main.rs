use std::{io, net, str::FromStr};

use actix::{prelude::*, spawn};
use bytes::BytesMut;
use serde::{Deserialize, Serialize};
use serde_json::Number;
use tokio::{
    io::{split, WriteHalf},
    net::{TcpListener, TcpStream},
};
use tokio_util::codec::{BytesCodec, FramedRead, LinesCodec, LinesCodecError};

#[derive(Serialize, Deserialize, Debug)]
struct Request {
    method: String,
    number: Number,
}

#[derive(Serialize, Deserialize, Debug)]
struct Response {
    method: String,
    prime: bool,
}

struct PrimeCheckSession {
    framed: actix::io::FramedWrite<BytesMut, WriteHalf<TcpStream>, BytesCodec>,
}

impl Actor for PrimeCheckSession {
    type Context = Context<Self>;

    fn stopping(&mut self, ctx: &mut Self::Context) -> Running {
        println!("Connection stopping...");
        Running::Stop
    }

    fn stopped(&mut self, ctx: &mut Self::Context) {
        println!("Connection stopped...");
    }
}

fn is_prime(num: i64) -> bool {
    if num < 2 {
        return false;
    }

    let mut check = 2;
    while check * check <= num {
        if num % check == 0 {
            return false;
        }
        check += 1;
    }
    return true;
}

impl StreamHandler<Result<String, LinesCodecError>> for PrimeCheckSession {
    fn finished(&mut self, ctx: &mut Self::Context) {
        self.framed.close();
    }

    fn handle(&mut self, item: Result<String, LinesCodecError>, ctx: &mut Self::Context) {
        match item {
            Ok(str) => {
                println!("Received request: {:?}", str);
                let decoded: Request = match serde_json::from_str(&str) {
                    Ok(x) => x,
                    Err(e) => {
                        println!("Err: {}", e);
                        self.framed.write(BytesMut::from("invalid json"));
                        ctx.stop();
                        return;
                    }
                };
                if decoded.method != "isPrime" {
                    self.framed.write(BytesMut::from("invalid method"));
                    ctx.stop();
                    return;
                }
                println!("Received json: {:?}", decoded);

                let prime = if let Some(num) = decoded.number.as_i64() {
                    is_prime(num)
                } else {
                    false
                };

                self.framed.write(BytesMut::from(
                    serde_json::to_string(&Response {
                        method: "isPrime".to_owned(),
                        prime,
                    })
                    .unwrap()
                    .as_str(),
                ));
                self.framed.write(BytesMut::from("\n"));
            }
            Err(e) => {
                println!("IO Error: {}", e);
                ctx.stop();
            }
        }
    }
}

impl actix::io::WriteHandler<io::Error> for PrimeCheckSession {}

impl PrimeCheckSession {
    fn new(
        framed: actix::io::FramedWrite<BytesMut, WriteHalf<TcpStream>, BytesCodec>,
    ) -> PrimeCheckSession {
        PrimeCheckSession { framed }
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
            PrimeCheckSession::create(|ctx| {
                let (r, w) = split(stream);
                let line_framed = FramedRead::new(r, LinesCodec::new_with_max_length(1024 * 1024));

                PrimeCheckSession::add_stream(line_framed, ctx);
                PrimeCheckSession::new(actix::io::FramedWrite::new(w, BytesCodec::new(), ctx))
            });
        }
    });
}

fn main() {
    let system = actix::System::new();

    let _addr = system.block_on(async { tcp_server("0.0.0.0:12345") });

    system.run().unwrap();
}
