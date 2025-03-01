use std::{env::args, process::exit};

const PORT: u16 = 6235;

pub fn wrap_command(command: &str) {
    let args = args().skip(1).collect::<Vec<_>>();
    let client = reqwest::blocking::Client::new();
    let response = client
        .post(format!("http://127.0.0.1:{}/run", PORT))
        .json(&ccelerate_shared::RunRequestData {
            binary: command.to_string(),
            args: args,
            cwd: std::env::current_dir().unwrap(),
        })
        .send();
    match response {
        Ok(_) => {
            exit(0);
        }
        Err(err) => {
            if err.is_connect() {
                eprintln!(
                    "Cannot connect to ccelerate_server on port {}, is it running?",
                    PORT
                );
            } else {
                eprintln!("Failed: {}", err);
            }
            exit(1);
        }
    }
}
