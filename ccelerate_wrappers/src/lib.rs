use std::{io::Write, process::exit};

pub fn wrap_command(wrapped_binary: ccelerate_shared::WrappedBinary) {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let client = reqwest::blocking::Client::builder()
        .timeout(None)
        .build()
        .unwrap();
    let response = client
        .post(format!(
            "http://127.0.0.1:{}/run",
            ccelerate_shared::DEFAULT_PORT
        ))
        .json(
            &ccelerate_shared::RunRequestData {
                binary: wrapped_binary,
                args: args,
                cwd: std::env::current_dir().unwrap(),
            }
            .to_wire(),
        )
        .send();
    match response {
        Ok(response) => {
            if !response.status().is_success() {
                println!(
                    "Failed to run command (status: {}): {}",
                    response.status(),
                    response.text().unwrap_or("Unknown error".to_string()),
                );
                exit(1);
            }
            let data = response
                .json::<ccelerate_shared::RunResponseDataWire>()
                .unwrap();
            let Ok(data) = ccelerate_shared::RunResponseData::from_wire(data) else {
                println!("Failed to decode response");
                exit(1);
            };
            std::io::stdout().write_all(&data.stdout).unwrap();
            std::io::stderr().write_all(&data.stderr).unwrap();
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
