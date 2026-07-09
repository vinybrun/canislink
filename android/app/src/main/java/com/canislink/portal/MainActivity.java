package com.canislink.portal;

import android.Manifest;
import android.annotation.SuppressLint;
import android.content.pm.PackageManager;
import android.os.Bundle;
import android.util.Log;
import android.webkit.JavascriptInterface;
import android.webkit.PermissionRequest;
import android.webkit.WebChromeClient;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import androidx.annotation.NonNull;
import androidx.appcompat.app.AppCompatActivity;
import androidx.core.app.ActivityCompat;
import androidx.core.content.ContextCompat;

/**
 * Phone = dog video terminal (camera + screen).
 * WebView loads portal; CanisBridge logs events for adb logcat e2e.
 */
public class MainActivity extends AppCompatActivity {
    private static final String TAG = "CanisLink";
    private static final int REQ = 42;
    private WebView web;

    public class CanisBridge {
        @JavascriptInterface
        public void log(String msg) {
            Log.i(TAG, "portal: " + msg);
        }

        @JavascriptInterface
        public void event(String name, String detail) {
            Log.i(TAG, "event name=" + name + " detail=" + detail);
        }
    }

    @SuppressLint("SetJavaScriptEnabled")
    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        web = new WebView(this);
        setContentView(web);

        WebSettings s = web.getSettings();
        s.setJavaScriptEnabled(true);
        s.setDomStorageEnabled(true);
        s.setMediaPlaybackRequiresUserGesture(false);
        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.LOLLIPOP) {
            s.setMixedContentMode(WebSettings.MIXED_CONTENT_ALWAYS_ALLOW);
        }
        WebView.setWebContentsDebuggingEnabled(true);
        s.setAllowFileAccess(true);
        s.setAllowContentAccess(true);

        web.addJavascriptInterface(new CanisBridge(), "CanisBridge");
        web.setWebViewClient(new WebViewClient() {
            @Override
            public void onPageFinished(WebView view, String url) {
                Log.i(TAG, "page_finished url=" + url);
                // bridge portal log() to Android
                String safeUrl = url.replace("\\", "\\\\").replace("'", "\\'");
                view.evaluateJavascript(
                    "(function(){if(window.CanisBridge){var el=document.getElementById('log');"
                    + "var obs=new MutationObserver(function(){CanisBridge.log(el?el.textContent.slice(0,300):'');});"
                    + "if(el)obs.observe(el,{childList:true,characterData:true,subtree:true});"
                    + "CanisBridge.event('page_ready','" + safeUrl + "');}})();",
                    null
                );
            }
        });
        web.setWebChromeClient(new WebChromeClient() {
            @Override
            public void onPermissionRequest(final PermissionRequest request) {
                // Grant WebRTC camera/mic capture inside WebView (after runtime perms).
                runOnUiThread(() -> {
                    String[] res = request.getResources();
                    StringBuilder sb = new StringBuilder();
                    for (String r : res) {
                        if (sb.length() > 0) sb.append(',');
                        sb.append(r);
                    }
                    Log.i(TAG, "webview_permission_request resources=" + sb);
                    request.grant(res);
                    Log.i(TAG, "webview_permission_granted");
                });
            }
            @Override
            public boolean onConsoleMessage(android.webkit.ConsoleMessage consoleMessage) {
                Log.i(TAG, "console: " + consoleMessage.message());
                return true;
            }
        });

        ensurePerms();
        String url = getIntent().getStringExtra("portal_url");
        if (url == null || url.isEmpty()) {
            // Prefer loopback (secure context for getUserMedia) when host used adb reverse.
            // Fallback: 10.0.2.2 is the emulator→host alias but is NOT a secure context.
            url = "http://127.0.0.1:18080/portal/";
        }
        Log.i(TAG, "loading " + url);
        Log.i(TAG, "hint: use adb reverse tcp:18080 tcp:18080 for getUserMedia secure context");
        web.loadUrl(url);
    }

    private void ensurePerms() {
        String[] need = {Manifest.permission.CAMERA, Manifest.permission.RECORD_AUDIO};
        boolean missing = false;
        for (String p : need) {
            if (ContextCompat.checkSelfPermission(this, p) != PackageManager.PERMISSION_GRANTED) {
                missing = true;
                break;
            }
        }
        if (missing) {
            ActivityCompat.requestPermissions(this, need, REQ);
        }
    }

    @Override
    public void onRequestPermissionsResult(int requestCode, @NonNull String[] permissions, @NonNull int[] grantResults) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults);
        if (requestCode == REQ && web != null) web.reload();
    }

    @Override
    protected void onDestroy() {
        if (web != null) web.destroy();
        super.onDestroy();
    }
}
