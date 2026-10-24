//! Identify the actual JS host inside the executing WASM, not from runner argv.

pub fn assert_execution_environment() {
    let global = js_sys::global();
    let window = js_sys::Reflect::get(&global, &"window".into()).expect("window inspection");
    if window.is_object() {
        let document = js_sys::Reflect::get(&global, &"document".into()).expect("browser document");
        let node_type =
            js_sys::Reflect::get(&document, &"nodeType".into()).expect("document nodeType");
        assert_eq!(
            node_type.as_f64(),
            Some(9.0),
            "real browser Document required"
        );
        let navigator =
            js_sys::Reflect::get(&global, &"navigator".into()).expect("browser navigator");
        let user_agent = js_sys::Reflect::get(&navigator, &"userAgent".into())
            .expect("browser userAgent")
            .as_string()
            .expect("userAgent string");
        assert!(user_agent.contains("Chrome/"), "OA-13 requires Chromium");
        wasm_bindgen_test::console_log!("OA13_BROWSER_CONTEXT document=9 user_agent={user_agent}");
    } else {
        let process = js_sys::Reflect::get(&global, &"process".into()).expect("Node process");
        let versions = js_sys::Reflect::get(&process, &"versions".into()).expect("Node versions");
        let node = js_sys::Reflect::get(&versions, &"node".into())
            .expect("Node version")
            .as_string()
            .expect("Node version string");
        assert!(
            !node.is_empty(),
            "Node runtime required outside the browser"
        );
        wasm_bindgen_test::console_log!("OA13_NODE_CONTEXT version={node}");
    }
}
