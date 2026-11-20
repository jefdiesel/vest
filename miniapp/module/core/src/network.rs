//! The chain the module talks to: a `names-network` file (format 1 or 2, the network file spec
//! the Logos apps share), or the built-in profile of the public LEZ rc3 demo chain.
//!
//! The file names the chain id, the RPC and the program ids (`programs.vesting`,
//! `programs.token`, `programs.lns_registry`; base58 or hex). Settings may override single
//! fields; a non-loopback RPC is always checked against the chain id (`getChannelId`) before
//! anything is read or sent.

use serde_json::Value;
use vesting_core::AccountId;
use vesting_sdk::names::parse_address;

/// The built-in profile: the public rc3 demo chain and this program's deployment on it.
pub const BUILT_IN: &str = include_str!("../network/rc3-demo.json");

/// The wire families this build speaks (`lez_release.wire`).
const WIRES: &[&str] = &["rc3"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Network {
    pub name: String,
    pub chain_id: String,
    pub rpc: String,
    pub program: Option<AccountId>,
    pub token_program: Option<AccountId>,
    pub lns_registry: Option<AccountId>,
    /// "built-in" or the file it came from.
    pub source: String,
}

fn program(file: &Value, role: &str) -> Result<Option<AccountId>, String> {
    match file.pointer(&format!("/programs/{role}/id")) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(id)) => parse_address(id)
            .map(Some)
            .ok_or_else(|| format!("programs.{role}.id '{id}' is not a 32-byte account")),
        Some(_) => Err(format!("programs.{role}.id must be a string")),
    }
}

/// Read a network file. Refuses a file for another LEZ line or a wire this build can't speak.
pub fn parse(text: &str, source: &str) -> Result<Network, String> {
    let file: Value =
        serde_json::from_str(text).map_err(|e| format!("network file {source}: {e}"))?;
    let format = file.get("format").and_then(Value::as_str).unwrap_or("");
    if !matches!(format, "names-network/1" | "names-network/2") {
        return Err(format!(
            "network file {source}: format '{format}' is not names-network/1 or /2"
        ));
    }
    let lez = file.get("lez").and_then(Value::as_str).unwrap_or("");
    if lez != "0.4" {
        return Err(format!(
            "network file {source}: LEZ {lez} (this build speaks LEZ 0.4 only)"
        ));
    }
    if let Some(wire) = file.pointer("/lez_release/wire").and_then(Value::as_str) {
        if !WIRES.contains(&wire) {
            return Err(format!(
                "this build can't use this network (wire '{wire}'); update the Vesting module"
            ));
        }
    }
    let text_field = |key: &str| {
        file.get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string()
    };
    // A v1 file has only the `registry` mirror for the LNS registry.
    let lns_registry = match program(&file, "lns_registry")? {
        Some(id) => Some(id),
        None => file
            .get("registry")
            .and_then(Value::as_str)
            .and_then(parse_address),
    };
    Ok(Network {
        name: text_field("name"),
        chain_id: text_field("chain_id").to_lowercase(),
        rpc: text_field("rpc"),
        program: program(&file, "vesting")?,
        token_program: program(&file, "token")?,
        lns_registry,
        source: source.to_string(),
    })
}

pub fn built_in() -> Network {
    parse(BUILT_IN, "built-in").expect("the built-in network profile parses")
}

/// `http://127.0.0.1…` / `http://localhost…`: a local test chain, allowed without a chain id.
pub fn is_loopback(rpc: &str) -> bool {
    let rest = rpc
        .strip_prefix("http://")
        .or_else(|| rpc.strip_prefix("https://"))
        .unwrap_or(rpc);
    rest.starts_with("127.0.0.1") || rest.starts_with("localhost") || rest.starts_with("[::1]")
}

/// The chain id an RPC serves (`getChannelId`), lower-case hex.
pub fn channel_id(client: &vesting_sdk::names::RpcClient) -> Result<String, String> {
    let answer = client
        .call("getChannelId", serde_json::json!([]))
        .map_err(|e| format!("getChannelId: {e}"))?;
    match answer {
        Value::String(id) => Ok(id.trim_start_matches("0x").to_lowercase()),
        Value::Array(bytes) => Ok(bytes
            .iter()
            .filter_map(Value::as_u64)
            .map(|b| format!("{:02x}", b as u8))
            .collect()),
        other => Err(format!("getChannelId: unexpected answer {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_profile_names_this_deployment() {
        let net = built_in();
        assert_eq!(net.rpc, "https://lez.nameswallet.app/");
        assert_eq!(net.chain_id.len(), 64);
        assert_eq!(
            hex::encode(net.program.unwrap()),
            "1a0cd691626eed61f82296bfd9d89609453a8fa8c947994a70dd1cbaa661327d"
        );
        assert_eq!(
            hex::encode(net.token_program.unwrap()),
            "d3116a21a92edc916e96a757c8cb4712018d9be7456e4f2e9bdefcac51661ea5"
        );
        assert!(net.lns_registry.is_some());
    }

    #[test]
    fn a_v1_file_and_refusals() {
        let v1 = r#"{"format":"names-network/1","lez":"0.4","chain_id":"AB","rpc":"https://x/",
                     "registry":"6Yxj5k8AiuBSuMAYVQFGrYKjJGUSSaWWgoB9D9XXN3gA"}"#;
        let net = parse(v1, "v1.json").unwrap();
        assert_eq!(net.chain_id, "ab");
        assert!(net.lns_registry.is_some());
        assert_eq!(net.program, None);
        let other_line = r#"{"format":"names-network/2","lez":"0.5"}"#;
        assert!(parse(other_line, "x").unwrap_err().contains("0.4 only"));
        let other_wire = r#"{"format":"names-network/1","lez":"0.4","lez_release":{"wire":"0.4"}}"#;
        assert!(parse(other_wire, "x").unwrap_err().contains("can't use"));
        assert!(is_loopback("http://127.0.0.1:3071"));
        assert!(!is_loopback("https://lez.nameswallet.app/"));
    }
}
