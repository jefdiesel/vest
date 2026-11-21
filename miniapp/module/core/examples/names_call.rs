//! Prints the Names Wallet `propose_call` arguments the module makes, read from the network in
//! the built-in profile: what the app hands to Names. For the end-to-end check of Names 0.18
//! (a create that reads the network clock).
//!
//! `cargo run -p vesting-module-core --example names_call -- create '<accounts json>' '<create row json>'`
//! `cargo run -p vesting-module-core --example names_call -- cancel '<accounts json>' <schedule account>`

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [what, accounts, arg] = args.as_slice() else {
        eprintln!("usage: names_call create|cancel|irrevocable <accounts json> <row json | schedule account>");
        std::process::exit(2);
    };
    let dir = std::env::temp_dir().join(format!("vest-names-call-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a temp dir");
    let mut module = vesting_module_core::Module::new(&dir);
    println!("{}", module.configure(r#"{"signer":"names"}"#));
    println!("{}", module.set_accounts(accounts));
    let answer = match what.as_str() {
        "create" => module.create_call(arg),
        "cancel" | "irrevocable" => {
            let imported: serde_json::Value =
                serde_json::from_str(&module.import_schedule(arg)).expect("JSON");
            let id = imported["id"].as_u64().unwrap_or_else(|| panic!("not imported: {imported}"));
            if what == "cancel" { module.cancel_call(id) } else { module.irrevocable_call(id) }
        }
        other => panic!("unknown call {other}"),
    };
    println!("{answer}");
    let _ = std::fs::remove_dir_all(&dir);
}
