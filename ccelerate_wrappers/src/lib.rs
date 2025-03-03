use base64::prelude::*;
use std::{env::args, io::Write, process::exit};

pub fn wrap_command(command: &str) {
    let args = args().skip(1).collect::<Vec<_>>();
    let client = reqwest::blocking::Client::builder()
        .timeout(None)
        .build()
        .unwrap();
    let response = client
        .post(format!(
            "http://127.0.0.1:{}/run",
            ccelerate_shared::DEFAULT_PORT
        ))
        .json(&ccelerate_shared::RunRequestData {
            binary: command.to_string(),
            args: args,
            cwd: std::env::current_dir().unwrap(),
        })
        .send();
    match response {
        Ok(response) => {
            if !response.status().is_success() {
                println!("Failed to run command");
                exit(1);
            }
            let data = response
                .json::<ccelerate_shared::RunResponseData>()
                .unwrap();
            let Ok(stdout) = BASE64_STANDARD.decode(&data.stdout) else {
                println!("Failed to decode stdout");
                exit(1);
            };
            let Ok(stderr) = BASE64_STANDARD.decode(&data.stderr) else {
                println!("Failed to decode stderr");
                exit(1);
            };
            std::io::stdout().write_all(&stdout).unwrap();
            std::io::stderr().write_all(&stderr).unwrap();
            exit(data.status);
        }
        Err(err) => {
            if err.is_connect() {
                println!(
                    "Cannot connect to ccelerate_server on port {}, is it running?",
                    ccelerate_shared::DEFAULT_PORT
                );
            } else if err.is_timeout() {
                println!("Connection to ccelerate_server timed out");
            } else {
                println!("Failed: {}", err);
            }
            exit(1);
        }
    }
}
