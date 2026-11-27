use std::path::PathBuf;

use guided_review::agent_cli::AgentCli;
use guided_review::build::Build;

#[test]
fn an_agent_runs_the_release_install_by_name_and_any_other_build_by_its_full_path() {
    let binary = PathBuf::from("/Users/me/.cargo/bin/guided-review");

    let release = AgentCli::for_build(Build::Release, binary.clone());
    let local = AgentCli::for_build(Build::Local, binary.clone());
    let test = AgentCli::for_build(Build::Test, binary);

    assert_eq!(release.to_string(), "guided-review");
    assert_eq!(local.to_string(), "/Users/me/.cargo/bin/guided-review");
    assert_eq!(test.to_string(), "/Users/me/.cargo/bin/guided-review");
}

#[test]
fn a_path_a_shell_would_split_is_quoted() {
    let cli = AgentCli::At(PathBuf::from("/Users/me/my builds/guided-review"));

    assert_eq!(cli.to_string(), "'/Users/me/my builds/guided-review'");
}
