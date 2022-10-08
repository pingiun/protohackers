use std::{borrow::Borrow, net, str::FromStr};

use actix::{prelude::*, spawn};
use futures::future::join_all;
use futures::stream::FuturesUnordered;
use slotmap::{new_key_type, SecondaryMap, SlotMap};
use tokio::{
    io::{split, WriteHalf},
    net::{TcpListener, TcpStream},
};
use tokio_util::codec::{FramedRead, LinesCodec, LinesCodecError};

#[derive(Debug, Eq, PartialEq)]
enum SessionState {
    Initial,
    Joined { key: ClientKey },
}
struct ChatSession {
    framed: actix::io::FramedWrite<String, WriteHalf<TcpStream>, LinesCodec>,
    server: Addr<ChatServer>,
    state: SessionState,
}

impl ChatSession {
    fn joined(&mut self, key: ClientKey) {
        self.state = SessionState::Joined { key }
    }
}

impl Actor for ChatSession {
    type Context = Context<Self>;

    fn stopping(&mut self, _ctx: &mut Self::Context) -> Running {
        println!("Connection stopping...");
        Running::Stop
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        println!("Connection stopped...");
    }

    fn started(&mut self, _ctx: &mut Self::Context) {
        self.framed
            .write("Welcome to budgetchat! What shall I call you?".to_string());
    }
}

impl StreamHandler<Result<String, LinesCodecError>> for ChatSession {
    fn finished(&mut self, ctx: &mut Self::Context) {
        if let SessionState::Joined { key } = &self.state {
            println!("Sending leave message");
            self.server
                .send(LeaveMessage(*key))
                .into_actor(self)
                .then(|res, _act, ctx| {
                    res.unwrap();
                    ctx.stop();
                    actix::fut::ready(())
                })
                .wait(ctx);
        }
        self.framed.close();

    }

    fn handle(&mut self, item: Result<String, LinesCodecError>, ctx: &mut Self::Context) {
        let line = match item {
            Ok(line) => line.trim_end().to_string(),
            Err(LinesCodecError::MaxLineLengthExceeded) => {
                eprintln!("Line too long");
                ctx.stop();
                return;
            }
            Err(_) => {
                eprintln!("IO Error");
                ctx.stop();
                return;
            }
        };
        match self.state {
            SessionState::Initial => {
                if line.len() < 1 {
                    eprintln!("Name too short");
                    ctx.stop();
                    return;
                }
                if !line.chars().all(char::is_alphanumeric) {
                    eprintln!("Invalid name");
                    ctx.stop();
                    return;
                }
                self.server
                    .send(JoinMessage {
                        addr: ctx.address().recipient::<ChatMessage>(),
                        name: line.clone(),
                    })
                    .into_actor(self)
                    .then(|res, act, _ctx| {
                        act.joined(res.unwrap());
                        actix::fut::ready(())
                    })
                    .wait(ctx);
            }
            SessionState::Joined { key } => {
                self.server
                    .send(SendMessage {
                        from: key,
                        msg: line,
                    })
                    .into_actor(self)
                    .then(|_res, _act, _ctx| actix::fut::ready(()))
                    .wait(ctx);
            }
        }
    }
}

impl Handler<ChatMessage> for ChatSession {
    type Result = ();

    fn handle(&mut self, msg: ChatMessage, _sctx: &mut Self::Context) -> Self::Result {
        self.framed.write(msg.0);
    }
}

impl actix::io::WriteHandler<LinesCodecError> for ChatSession {}

impl ChatSession {
    fn new(
        framed: actix::io::FramedWrite<String, WriteHalf<TcpStream>, LinesCodec>,
        server: Addr<ChatServer>,
    ) -> ChatSession {
        ChatSession {
            framed,
            server,
            state: SessionState::Initial,
        }
    }
}

new_key_type! {
    #[derive(Message)]
    #[rtype(result = "()")]
    struct ClientKey;
}
struct ChatServer {
    clients: SlotMap<ClientKey, Recipient<ChatMessage>>,
    names: SecondaryMap<ClientKey, String>,
}

#[derive(Debug, Message)]
#[rtype(result = "()")]
struct ChatMessage(String);

#[derive(Debug, Message)]
#[rtype(result = "ClientKey")]
struct JoinMessage {
    addr: Recipient<ChatMessage>,
    name: String,
}

#[derive(Debug, Message)]
#[rtype(result = "()")]
struct LeaveMessage(ClientKey);

#[derive(Debug, Message)]
#[rtype(result = "()")]
struct SendMessage {
    from: ClientKey,
    msg: String,
}

impl Actor for ChatServer {
    type Context = Context<Self>;
}

impl Handler<SendMessage> for ChatServer {
    type Result = ();

    fn handle(&mut self, msg: SendMessage, ctx: &mut Self::Context) -> Self::Result {
        let name = self.names.get(msg.from).unwrap();
        let futures = FuturesUnordered::new();
        for (key, addr) in self.clients.iter() {
            if key == msg.from {
                continue;
            }
            futures.push(Box::pin(
                addr.send(ChatMessage(format!("[{}] {}", name, msg.msg))),
            ));
        }
        join_all(futures)
            .into_actor(self)
            .then(|_res, _act, _ctx| actix::fut::ready(()))
            .wait(ctx);
    }
}

impl Handler<JoinMessage> for ChatServer {
    type Result = MessageResult<JoinMessage>;

    fn handle(&mut self, msg: JoinMessage, ctx: &mut Self::Context) -> Self::Result {
        let futures = FuturesUnordered::new();
        for (_key, addr) in self.clients.iter() {
            futures.push(Box::pin(
                addr.send(ChatMessage(format!("* {} has entered the room", msg.name))),
            ));
        }
        futures.push(Box::pin(msg.addr.send(ChatMessage(format!(
            "* The room contains: {}",
            self.names.values().map(|x| x.borrow()).collect::<Vec<&str>>().join(", ")
        )))));
        let key = self.clients.insert(msg.addr);
        self.names.insert(key, msg.name);
        join_all(futures)
            .into_actor(self)
            .then(|_res, _act, _ctx| actix::fut::ready(()))
            .wait(ctx);
        MessageResult(key)
    }
}

impl Handler<LeaveMessage> for ChatServer {
    type Result = ();

    fn handle(&mut self, msg: LeaveMessage, ctx: &mut Self::Context) -> Self::Result {
        self.clients.remove(msg.0);
        let name = self.names.remove(msg.0).unwrap();
        println!("Leave message received from {}", name);
        let futures = FuturesUnordered::new();
        for (_key, addr) in self.clients.iter() {
            futures.push(Box::pin(
                addr.send(ChatMessage(format!("* {} has left the room", name))),
            ));
        }
        join_all(futures)
            .into_actor(self)
            .then(|_res, _act, _ctx| actix::fut::ready(()))
            .wait(ctx);
    }
}

pub fn tcp_server(s: &str) {
    // Create server listener
    let addr = net::SocketAddr::from_str(s).unwrap();

    spawn(async move {
        let listener = TcpListener::bind(&addr).await.unwrap();
        let server = ChatServer::create(|_ctx| {
            return ChatServer {
                clients: SlotMap::<ClientKey, _>::with_key(),
                names: SecondaryMap::<ClientKey, _>::new(),
            };
        });

        while let Ok((stream, addr)) = listener.accept().await {
            println!("Connection from: {}", addr);
            // let server = server.clone();
            ChatSession::create(|ctx| {
                let (r, w) = split(stream);
                let linescodes = LinesCodec::new_with_max_length(2048);

                ChatSession::add_stream(FramedRead::new(r, linescodes), ctx);
                ChatSession::new(
                    actix::io::FramedWrite::new(w, LinesCodec::new(), ctx),
                    server.clone(),
                )
            });
        }
    });
}

fn main() {
    let system = actix::System::new();

    system.block_on(async { tcp_server("0.0.0.0:12345") });

    system.run().unwrap();
}
