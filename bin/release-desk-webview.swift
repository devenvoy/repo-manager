import Cocoa
import WebKit

class WebviewAppDelegate: NSObject, NSApplicationDelegate, WKNavigationDelegate {
    var window: NSWindow!
    var webView: WKWebView!
    var targetUrl: URL!
    var callbackPrefix: String!

    init(url: URL, callback: String) {
        self.targetUrl = url
        self.callbackPrefix = callback
        super.init()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        let rect = NSRect(x: 0, y: 0, width: 640, height: 720)
        window = NSWindow(contentRect: rect,
                          styleMask: [.titled, .closable, .miniaturizable, .resizable],
                          backing: .buffered,
                          defer: false)
        window.center()
        window.title = "Release Desk • Authentication"
        
        let config = WKWebViewConfiguration()
        webView = WKWebView(frame: rect, configuration: config)
        webView.navigationDelegate = self
        webView.autoresizingMask = [.width, .height]
        window.contentView = webView
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        
        webView.load(URLRequest(url: targetUrl))
    }

    func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction, decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
        if let url = navigationAction.request.url?.absoluteString {
            if !callbackPrefix.isEmpty && url.starts(with: callbackPrefix) {
                decisionHandler(.allow)
                DispatchQueue.main.asyncAfter(deadline: .now() + 1.0) {
                    NSApp.terminate(nil)
                }
                return
            }
        }
        decisionHandler(.allow)
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        return true
    }
}

let args = CommandLine.arguments
guard args.count >= 2 else {
    print("Usage: release-desk-webview <url> [callback_prefix]")
    exit(1)
}

guard let targetUrl = URL(string: args[1]) else {
    print("Invalid URL: \(args[1])")
    exit(1)
}

let callbackPrefix = args.count >= 3 ? args[2] : ""

let app = NSApplication.shared
let delegate = WebviewAppDelegate(url: targetUrl, callback: callbackPrefix)
app.delegate = delegate
app.setActivationPolicy(.regular)
app.run()
