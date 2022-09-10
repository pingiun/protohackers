use std::ops::Bound::Included;
use std::{collections::BTreeMap, convert::TryInto, io, net, str::FromStr};

use actix::{prelude::*, spawn};
use bytes::{BytesMut, BufMut};
use tokio::{
    io::{split, WriteHalf},
    net::{TcpListener, TcpStream},
};
use tokio_util::codec::{Decoder, Encoder, FramedRead};

enum Request {
    Insert { time: i32, pennies: i32 },
    Query { mintime: i32, maxtime: i32 },
}

type Response = i32;

struct PrimeCheckSession {
    data: BTreeMap<i32, i32>,
    framed: actix::io::FramedWrite<Response, WriteHalf<TcpStream>, AnswerEncoder>,
}

impl Actor for PrimeCheckSession {
    type Context = Context<Self>;

    fn stopping(&mut self, _ctx: &mut Self::Context) -> Running {
        println!("Connection stopping...");
        Running::Stop
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        println!("Connection stopped...");
    }
}

impl StreamHandler<Result<Request, io::Error>> for PrimeCheckSession {
    fn finished(&mut self, _ctx: &mut Self::Context) {
        self.framed.close();
    }

    fn handle(&mut self, item: Result<Request, io::Error>, ctx: &mut Self::Context) {
        match item {
            Ok(Request::Insert { time, pennies }) => {
                println!("Insert at {} value {}", time, pennies);
                self.data.insert(time, pennies);
            }
            Ok(Request::Query { mintime, maxtime }) => {
                println!("Query from {} to {}", mintime, maxtime);
                if mintime > maxtime {
                    self.framed.write(0);
                    return;
                }
                let mut sum: i64 = 0;
                let mut items = 0;
                for (_, value) in self.data.range((Included(mintime), Included(maxtime))) {
                    sum += *value as i64;
                    items += 1;
                }
                self.framed.write(sum.checked_div(items).unwrap_or(0).try_into().unwrap_or(0));
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
        framed: actix::io::FramedWrite<Response, WriteHalf<TcpStream>, AnswerEncoder>,
    ) -> PrimeCheckSession {
        PrimeCheckSession {
            framed,
            data: BTreeMap::new(),
        }
    }
}

struct NineBytesDecoder;

impl Decoder for NineBytesDecoder {
    type Item = Request;

    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() < 9 {
            return Ok(None);
        }
        let bytes = src.split_to(9);

        let first = i32::from_be_bytes(bytes[1..5].try_into().unwrap());
        let second = i32::from_be_bytes(bytes[5..9].try_into().unwrap());
        if bytes[0] == b'I' {
            Ok(Some(Request::Insert {
                time: first,
                pennies: second,
            }))
        } else if bytes[0] == b'Q' {
            Ok(Some(Request::Query {
                mintime: first,
                maxtime: second,
            }))
        } else {
            Err(io::Error::new(io::ErrorKind::Other, "invalid message type"))
        }
    }
}

struct AnswerEncoder;

impl Encoder<Response> for AnswerEncoder {
    type Error = io::Error;

    fn encode(&mut self, item: Response, dst: &mut BytesMut) -> Result<(), Self::Error> {
        dst.put(&i32::to_be_bytes(item)[..]);
        Ok(())
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

                PrimeCheckSession::add_stream(FramedRead::new(r, NineBytesDecoder), ctx);
                PrimeCheckSession::new(actix::io::FramedWrite::new(w, AnswerEncoder, ctx))
            });
        }
    });
}

fn main() {
    let system = actix::System::new();

    system.block_on(async { tcp_server("0.0.0.0:12345") });

    system.run().unwrap();
}
