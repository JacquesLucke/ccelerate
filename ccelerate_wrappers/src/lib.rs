#![deny(clippy::unwrap_used)]

use std::{io::Write, process::exit};

pub fn wrap_command(binary: ccelerate_shared::WrappedBinary) {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let Ok(client) = reqwest::blocking::Client::builder().timeout(None).build() else {
        eprintln!("Failed to build reqwest client");
        exit(1);
    };
    let Ok(cwd) = std::env::current_dir() else {
        eprintln!("Failed to get current directory");
        exit(1);
    };

    let request = ccelerate_shared::RunRequestData { binary, args, cwd };
    let response = client
        .post(format!(
            "http://127.0.0.1:{}/run",
            ccelerate_shared::DEFAULT_PORT
        ))
        .json(&request.to_wire())
        .send();
    match response {
        Ok(response) => {
            if !response.status().is_success() {
                eprintln!(
                    "Failed to run command (status: {}): {}",
                    response.status(),
                    response.text().unwrap_or("Unknown error".to_string()),
                );
                exit(1);
            }
            let Ok(data) = response.json::<ccelerate_shared::RunResponseDataWire>() else {
                eprintln!("Failed to decode response");
                exit(1);
            };
            let Ok(data) = ccelerate_shared::RunResponseData::from_wire(data) else {
                eprintln!("Failed to decode response");
                exit(1);
            };
            std::io::stdout().write_all(&data.stdout).ok();
            std::io::stderr().write_all(&data.stderr).ok();
            exit(data.status);
        }
        Err(err) => {
            if err.is_connect() {
                eprintln!(
                    "Cannot connect to ccelerate_server on port {}, is it running?",
                    ccelerate_shared::DEFAULT_PORT
                );
            } else if err.is_timeout() {
                eprintln!("Connection to ccelerate_server timed out");
            } else {
                eprintln!("Failed: {}", err);
            }
            exit(1);
        }
    }
}
