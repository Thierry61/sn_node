// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! sn_node provides the interface to Safe routing.  The resulting executable is the node
//! for the Safe network.

#![doc(
    html_logo_url = "https://raw.githubusercontent.com/maidsafe/QA/master/Images/maidsafe_logo.png",
    html_favicon_url = "https://maidsafe.net/img/favicon.ico",
    test(attr(forbid(warnings)))
)]
// For explanation of lint checks, run `rustc -W help`.
#![forbid(unsafe_code)]
#![warn(
    missing_debug_implementations,
    missing_docs,
    trivial_casts,
    trivial_numeric_casts,
    unused_extern_crates,
    unused_import_braces,
    unused_qualifications,
    unused_results
)]

use directories::BaseDirs;
use log::{debug, info};
use sn_launch_tool::run_with;
use std::{
    fs::{create_dir_all, remove_dir_all},
    path::PathBuf,
};
use tokio::time::{delay_for, Duration};

#[cfg(not(target_os = "windows"))]
const SAFE_NODE_EXECUTABLE: &str = "sn_node";

#[cfg(target_os = "windows")]
const SAFE_NODE_EXECUTABLE: &str = "sn_node.exe";

static NODES_DIR: &str = "local-test-network";
static INTERVAL: &str = "3";

#[tokio::main]
async fn main() -> Result<(), String> {
    let path = std::path::Path::new("nodes");
    remove_dir_all(&path).unwrap_or(()); // Delete nodes directory if it exists;
    create_dir_all(&path).expect("Cannot create nodes directory");

    let _ = run_network().await?;

    Ok(())
}

fn get_node_bin_path(node_path: Option<PathBuf>) -> Result<PathBuf, String> {
    match node_path {
        Some(p) => Ok(p),
        None => {
            let base_dirs =
                BaseDirs::new().ok_or_else(|| "Failed to obtain user's home path".to_string())?;

            let mut path = PathBuf::from(base_dirs.home_dir());
            path.push(".safe");
            path.push("node");
            Ok(path)
        }
    }
}

/// Uses SNLT to create a local network of nodes
pub async fn run_network() -> Result<(), String> {
    info!("Starting local network");
    let verbosity = 4;
    let node_path = Some(PathBuf::from("./target/release"));
    let node_path = get_node_bin_path(node_path)?;

    let arg_node_path = node_path.join(SAFE_NODE_EXECUTABLE).display().to_string();
    debug!("Running node from {}", arg_node_path);

    let base_log_dir = get_node_bin_path(None)?;
    let node_log_dir = base_log_dir.join(NODES_DIR);
    if !node_log_dir.exists() {
        debug!("Creating '{}' folder", node_log_dir.display());
        create_dir_all(node_log_dir.clone()).map_err(|err| {
            format!(
                "Couldn't create target path to store nodes' generated data: {}",
                err
            )
        })?;
    }
    let arg_node_log_dir = node_log_dir.display().to_string();
    info!("Storing nodes' generated data at {}", arg_node_log_dir);

    // Let's create an args array to pass to the network launcher tool
    let mut sn_launch_tool_args = vec![
        "sn_launch_tool",
        "-v",
        "--node-path",
        &arg_node_path,
        "--nodes-dir",
        &arg_node_log_dir,
        "--interval",
        &INTERVAL,
        "--local",
    ];

    let interval_as_int = &INTERVAL.parse::<u64>().unwrap();

    let mut verbosity_arg = String::from("-");
    if verbosity > 0 {
        let v = "y".repeat(verbosity as usize);
        info!("V: {}", v);
        verbosity_arg.push_str(&v);
        sn_launch_tool_args.push(&verbosity_arg);
    }

    debug!(
        "Running network launch tool with args: {:?}",
        sn_launch_tool_args
    );

    // We can now call the tool with the args
    info!("Launching local Safe network...");
    run_with(Some(&sn_launch_tool_args))?;

    let interval_duration = Duration::from_secs(interval_as_int * 15);

    delay_for(interval_duration).await;

    Ok(())
}
