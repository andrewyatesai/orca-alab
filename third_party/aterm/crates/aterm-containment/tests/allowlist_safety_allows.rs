// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Integration test: Safety mode with allowlist permits listed operations.
//!
//! Fresh OnceLock binary — initializes Safety mode + allowlist, then verifies
//! that listed MCP tools, plugins, network targets, and commands are allowed.
//! that listed MCP tools, plugins, network targets, and processes are allowed.

use aterm_containment::{AllowlistConfig, ContainmentMode};

#[test]
fn safety_mode_allows_listed_operations() {
    // Initialize Safety mode.
    aterm_containment::init_mode(ContainmentMode::Safety).expect("mode init");

    // Provide an allowlist with specific entries.
    let config = AllowlistConfig {
        mcp_tools: vec!["read_file".into(), "write_file".into()],
        plugins: vec!["spell-check".into()],
        network: vec!["localhost:*".into(), "unix:/tmp/aterm.sock".into()],
        processes: vec!["/bin/bash".into(), "/bin/zsh".into()],
    };
    aterm_containment::init_allowlist(config).expect("allowlist init");

    // Listed MCP tools are allowed.
    assert!(aterm_containment::is_mcp_allowed("read_file"));
    assert!(aterm_containment::is_mcp_allowed("write_file"));

    // Unlisted MCP tool is denied.
    assert!(!aterm_containment::is_mcp_allowed("execute_command"));

    // Listed plugin is allowed.
    assert!(aterm_containment::is_plugin_allowed("spell-check"));

    // Unlisted plugin is denied.
    assert!(!aterm_containment::is_plugin_allowed("evil-plugin"));

    // Listed network targets are allowed.
    assert!(aterm_containment::is_network_allowed("localhost:8080"));
    assert!(aterm_containment::is_network_allowed("localhost:443"));
    assert!(aterm_containment::is_network_allowed(
        "unix:/tmp/aterm.sock"
    ));

    // Unlisted network target is denied.
    assert!(!aterm_containment::is_network_allowed("example.com:80"));

    // Listed commands are allowed.
    assert!(aterm_containment::is_process_allowed("/bin/bash"));
    assert!(aterm_containment::is_process_allowed("/bin/zsh"));

    // Unlisted shell is denied.
    assert!(!aterm_containment::is_process_allowed("/bin/evil"));
}
