use guided_review::code_source::{CodeSource, parse_unified_diff};
use guided_review::pull_request::Sha;
use guided_review::viewer::CodeHost;

use crate::host_contract::{HostBuilder, PullOnHost, SeedCode};
use crate::support::file_path;

const CART_DIFF: &str = "diff --git a/cart.rs b/cart.rs
index 1111111..2222222 100644
--- a/cart.rs
+++ b/cart.rs
@@ -1,2 +1,2 @@
 pub struct Cart;
-pub fn total() {}
+pub fn total() -> u32 { 0 }
";

fn shop() -> PullOnHost {
    PullOnHost {
        base_branch_tip: "main-tip",
        head: "cart-head",
        code: SeedCode {
            files: vec![
                (
                    "main-tip",
                    "cart.rs",
                    "pub struct Cart;\npub fn total() {}\n",
                ),
                (
                    "cart-head",
                    "cart.rs",
                    "pub struct Cart;\npub fn total() -> u32 { 0 }\n",
                ),
                (
                    "cart-head",
                    "copy.rs",
                    "pub struct Cart;\npub fn total() -> u32 { 0 }\n",
                ),
            ],
            diff: CART_DIFF,
            merge_base: "fork-point",
        },
        ..PullOnHost::default()
    }
}

fn code_of(host: &dyn CodeHost) -> std::sync::Arc<dyn CodeSource> {
    host.code_of(&"acme/shop".parse().unwrap()).unwrap()
}

pub fn a_file_reads_at_each_commit_with_its_blob_info(host: HostBuilder) {
    let host = host(shop());
    let code = code_of(host.as_ref());
    let (base, head) = (Sha::new("main-tip"), Sha::new("cart-head"));
    let cart = file_path("cart.rs");

    let before = code.file_at(&base, &cart).unwrap();
    let after = code.file_at(&head, &cart).unwrap();
    let bytes = code.blob_at(&head, &cart).unwrap();
    let [info_before, info_after, info_copy] =
        [(&base, "cart.rs"), (&head, "cart.rs"), (&head, "copy.rs")]
            .map(|(sha, path)| code.blob_info_at(sha, &file_path(path)).unwrap());

    assert_eq!(before, "pub struct Cart;\npub fn total() {}\n");
    assert_eq!(after, "pub struct Cart;\npub fn total() -> u32 { 0 }\n");
    assert_eq!(&*bytes, after.as_bytes());
    assert_eq!(info_after.size, u64::try_from(after.len()).unwrap());
    assert_eq!(info_after.id, info_copy.id, "equal bytes have equal ids");
    assert_ne!(info_before.id, info_after.id);
    assert!(code.file_at(&head, &file_path("gone.rs")).is_err());
}

pub fn the_diff_and_merge_base_of_the_tips_read_as_the_host_has_them(host: HostBuilder) {
    let host = host(shop());
    let code = code_of(host.as_ref());
    let (base, head) = (Sha::new("main-tip"), Sha::new("cart-head"));

    let diff = code.diff(&base, &head).unwrap();
    let merge_base = code.merge_base(&base, &head).unwrap();

    assert_eq!(
        format!("{diff:?}"),
        format!("{:?}", parse_unified_diff(CART_DIFF).unwrap())
    );
    assert_eq!(merge_base, Sha::new("fork-point"));
}
