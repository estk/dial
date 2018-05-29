#[macro_use]
extern crate log;
extern crate env_logger;
extern crate futures;
extern crate tokio;
extern crate tokio_tcp;
extern crate trust_dns_resolver;

use std::net::IpAddr;
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use trust_dns_resolver::lookup_ip::LookupIp;

use futures::prelude::*;
use futures::sync::mpsc;

use tokio::prelude::*;
use tokio::timer::Delay;
use tokio_tcp::TcpStream;

use trust_dns_resolver::config::LookupIpStrategy;
use trust_dns_resolver::error::{ResolveError, ResolveResult};
use trust_dns_resolver::system_conf::read_system_conf;
use trust_dns_resolver::ResolverFuture;

fn main() {
    env_logger::init();
    // let lookup_future = dialer.resolve("www.google.com").unwrap();
    // let lookup_res = io_loop.block_on(lookup_future).unwrap();
    // println!("resolve: {:?}", lookup_res);

    tokio::run(
        dial("www.google.com", 80)
            .unwrap()
            .map_err(|e| eprintln!("error: {}", e))
            .and_then(|res| {
                println!("dial {:?}", res);
                futures::future::ok(())
            })
    );
}

pub fn dial(
    host: &'static str,
    port: u16,
) -> ResolveResult<Box<Future<Item = TcpStream, Error = Box<std::error::Error>> + Send>> {
    Dialer::default().dial(host, port)
}

/// Dialer uses happy eyeballs to resolve and connect to a host
/// The struct fields are named according to the happy eyeballs
/// rfc [rfc8305](https://tools.ietf.org/html/rfc8305#section-3)
#[derive(Debug, Clone, Copy)]
struct Dialer {
    pub prefer_v6: bool,
    pub resolution_delay: u16,
    pub first_addr_family_count: u8,
    pub conn_attempt_delay: u16,
    pub min_conn_delay: u16,
    pub max_conn_delay: u16,
    pub last_resort_local_synthesis_delay: u16,
}

/// Defaults as specified in the rfc
impl Default for Dialer {
    fn default() -> Self {
        Self {
            prefer_v6: true,
            resolution_delay: 50,       // unused
            first_addr_family_count: 1, // unused
            conn_attempt_delay: 250,
            min_conn_delay: 100,                     // unused
            max_conn_delay: 2000,                    // unused
            last_resort_local_synthesis_delay: 2000, // unused
        }
    }
}

impl Dialer {
    pub fn dial(
        self,
        host: &'static str,
        port: u16,
    ) -> ResolveResult<Box<Future<Item = TcpStream, Error = Box<std::error::Error>>+Send>> {
        debug!("dialing: {}, {}", host, port);

        let (v4, v6) = self.resolve(host)?;
        Ok(Box::new(future::lazy(move || {
            let ordered = self.order(v4, v6);
            let socks = ordered.map(move |ip| SocketAddr::new(ip, port));

            Box::new(self.connect(Box::new(socks)))
        })))
    }
    fn order(
        &self,
        v4: Box<Future<Item = LookupIp, Error = ResolveError> + Send>,
        v6: Box<Future<Item = LookupIp, Error = ResolveError> + Send>,
    ) -> Box<Stream<Item = IpAddr, Error = ()> + Send> {
        debug!("ordering");
        let (v6_tx, v6_rx) = mpsc::channel::<IpAddr>(1);
        let (v4_tx, v4_rx) = mpsc::channel::<IpAddr>(1);

        let (unblock_v4, blocked_v4) = mpsc::channel::<()>(1);
        let (unblock_v6, blocked_v6) = mpsc::channel::<()>(1);

        let start_unblock_v4 = unblock_v4.clone();
        let start_unblock_v6 = unblock_v6.clone();

        let prefer_v6 = self.prefer_v6;

        debug!("Spawning unblockers");
        tokio::spawn({
            let fut = if prefer_v6 {
                debug!("prefer_v6");
                start_unblock_v6.send(())
            } else {
                debug!("prefer_v4");
                start_unblock_v4.send(())
            };
            fut.map(|_| ())
                .map_err(|e| eprintln!("accept failed = {:?}", e))
        });

        let send_unblock_v4 = unblock_v4.clone();
        let send_unblock_v6 = unblock_v6.clone();

        debug!("Spawning v6 collector");
        tokio::spawn(
            v6.map_err(|e| eprintln!("accept failed = {:?}", e))
                .join(
                    blocked_v6
                        .take(1)
                        .collect()
                        .map_err(|e| eprintln!("accept failed = {:?}", e)),
                )
                .and_then(move |(lip, _): (LookupIp, _)| {
                    debug!("collecting v6");
                    let owned_ips = lip.iter().collect::<Vec<_>>();
                    send_unblock_v4.send(())
                        .map_err(|e| eprintln!("accept failed = {:?}", e))
                        .and_then(move |_| {
                            stream::iter_ok(owned_ips)
                                .and_then(move |ip| v6_tx.clone().send(ip))
                                .collect()
                                .map_err(|e| eprintln!("accept failed = {:?}", e))
                        })
                })
                .map(|_| ())
        );

        debug!("Spawning v4 collector");
        tokio::spawn(
            v4.map_err(|e| eprintln!("accept failed = {:?}", e))
                .join(
                    blocked_v4
                        .take(1)
                        .collect()
                        .map_err(|e| eprintln!("accept failed = {:?}", e)),
                )
                .and_then(move |(lip, _): (LookupIp, _)| {
                    let owned_ips = lip.iter().collect::<Vec<_>>();
                    send_unblock_v6.send(())
                        .map_err(|e| eprintln!("accept failed = {:?}", e))
                        .and_then(move |_| {
                            stream::iter_ok(owned_ips)
                                .and_then(move |ip| v4_tx.clone().send(ip))
                                .collect()
                                .map_err(|e| eprintln!("accept failed = {:?}", e))
                        })
                })
                .map(|_| ())
        );
        // interleave v4 and v6
        Box::new(v6_rx)
    }
    pub fn resolve(
        &self,
        host: &'static str,
    ) -> ResolveResult<(
        Box<Future<Item = LookupIp, Error = ResolveError>+Send>,
        Box<Future<Item = LookupIp, Error = ResolveError>+Send>,
    )> {
        debug!("resolving: {}", host);
        let (conf, opts) = read_system_conf()?;
        // read_system_conf just returns the ::default opts
        let mut v4_opts = opts.clone();
        v4_opts.ip_strategy = LookupIpStrategy::Ipv4Only;
        let v4_fut =
            ResolverFuture::new(conf.clone(), v4_opts).and_then(move |r| r.lookup_ip(host));

        let mut v6_opts = opts.clone();
        v6_opts.ip_strategy = LookupIpStrategy::Ipv6Only;
        let v6_fut =
            ResolverFuture::new(conf.clone(), v6_opts).and_then(move |r| r.lookup_ip(host));

        Ok((Box::new(v4_fut), Box::new(v6_fut)))
    }
    pub fn connect(
        &self,
        socks: Box<Stream<Item = SocketAddr, Error = ()> + Send>,
    ) -> Box<Future<Item = TcpStream, Error = Box<std::error::Error>>+Send> {
        debug!("Connect");
        let (tx, rx) = futures::sync::mpsc::channel(1);
        let conn_attempt_delay = self.conn_attempt_delay;
        tokio::spawn(stream::iter_ok(1..).zip(socks).for_each(move |(i, s)| {
            let offset = ((i as u64) + 1) * conn_attempt_delay as u64;
            let when = Instant::now() + Duration::from_millis(offset);
            let txc = tx.clone();
            debug!("creating delay of {}", offset);
            Delay::new(when)
                .map_err(|e| panic!("delay errored; err={:?}", e))
                .and_then(move |_| {
                    debug!("happy eyeballs start connect: {}", s);
                    TcpStream::connect(&s).map_err(|e| panic!("connect errored; err={:?}", e))
                })
                .and_then(move |c| {
                    debug!("happy eyeballs connected: {:?}", c);
                    txc.send(c)
                        .and_then(|_| Ok(()))
                        .map_err(|e| panic!("send errored; err={:?}", e))
                })
        }));
        Box::new(
            rx
                .into_future()
                .map_err(|e| panic!("recieve errored; err={:?}", e))
                .and_then(|x| {
                    match x.0 {
                        None => future::failed("Exhausted addrs"),
                        Some(tcps) => future::ok(tcps),
                    }
                })
                .map_err(|e| panic!("recieve errored; err={:?}", e))
        )
    }
}
