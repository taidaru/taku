use std::io;
use std::time::Duration;

use ureq::Agent;
use ureq::config::Config;

/// Connect timeout only. A *global* timeout would also cap body transfer,
/// which for `net.download` streaming a multi-gigabyte file to disk means any
/// download slower than the cap fails mid-stream — so we bound the handshake,
/// not the transfer.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// `net.get` buffers the body in memory (and hands it to Lua), so it
/// gets a deliberately modest cap; `net.download` streams to disk and only
/// caps against unbounded responses.
const GET_LIMIT: u64 = 64 * 1024 * 1024;
const DOWNLOAD_LIMIT: u64 = 8 * 1024 * 1024 * 1024;

fn config() -> Config {
    Config::builder()
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .build()
}

pub fn local_agent() -> Agent {
    Agent::new_with_config(config())
}

pub fn get(agent: &Agent, url: &str) -> Result<Vec<u8>, ureq::Error> {
    agent
        .get(url)
        .call()?
        .body_mut()
        .with_config()
        .limit(GET_LIMIT)
        .read_to_vec()
}

pub fn get_reader(agent: &Agent, url: &str) -> Result<impl io::Read + use<>, ureq::Error> {
    Ok(agent
        .get(url)
        .call()?
        .into_body()
        .into_with_config()
        .limit(DOWNLOAD_LIMIT)
        .reader())
}
