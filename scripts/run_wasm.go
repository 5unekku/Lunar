package main

import (
	"fmt"
	"log"
	"net"
	"net/http"
	"os"
	"os/exec"
	"os/signal"
	"path/filepath"
	"runtime"
	"syscall"
	"strings"
	"time"
)

func main() {
	root := repoRoot()

	if len(os.Args) < 2 {
		listTargets(root)
		os.Exit(0)
	}
	name := os.Args[1]

	// build
	fmt.Printf("building %s for wasm...\n", name)
	build := exec.Command("cargo", "build",
		"--target", "wasm32-unknown-unknown",
		"--example", name,
		"--release",
	)
	build.Dir = root
	build.Stdout = os.Stdout
	build.Stderr = os.Stderr
	if err := build.Run(); err != nil {
		log.Fatalf("cargo build failed: %v", err)
	}

	// wasm-bindgen
	wasmSrc := filepath.Join(root, "target", "wasm32-unknown-unknown", "release", "examples", name+".wasm")
	tmpDir, err := os.MkdirTemp("", "lunar-wasm-*")
	if err != nil {
		log.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	fmt.Println("running wasm-bindgen...")
	bindgen := exec.Command("wasm-bindgen", "--target", "web", "--out-dir", tmpDir, wasmSrc)
	bindgen.Stdout = os.Stdout
	bindgen.Stderr = os.Stderr
	if err := bindgen.Run(); err != nil {
		log.Fatalf("wasm-bindgen failed: %v", err)
	}

	// minimal index.html — canvas fits the viewport while preserving aspect ratio.
	// JS sets the canvas buffer size to match its CSS-rendered size so there is
	// no extra browser-level scaling. ResizeObserver keeps it in sync on resize.
	html := fmt.Sprintf(`<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>%s</title>
<style>
* { margin: 0; padding: 0; box-sizing: border-box; }
body { background: hsl(0, 0%%, 0%%); width: 100vw; height: 100vh; overflow: hidden; display: flex; align-items: center; justify-content: center; }
canvas { display: block; max-width: 100%%; max-height: 100%%; aspect-ratio: 1280 / 720; width: 100%%; }
</style>
</head>
<body>
<canvas id="lunar-canvas"></canvas>
<script>
(function() {
    var c = document.getElementById('lunar-canvas');
    function fit() {
        var w = Math.round(c.getBoundingClientRect().width);
        var h = Math.round(c.getBoundingClientRect().height);
        if (w > 0 && h > 0) { c.width = w; c.height = h; }
    }
    fit();
    new ResizeObserver(fit).observe(c);
})();
</script>
<script type="module">
import init from './%s.js';
await init();
</script>
</body>
</html>`, name, name)

	if err := os.WriteFile(filepath.Join(tmpDir, "index.html"), []byte(html), 0644); err != nil {
		log.Fatalf("failed to write index.html: %v", err)
	}

	// bind on a random free port
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		log.Fatalf("failed to bind: %v", err)
	}
	port := listener.Addr().(*net.TCPAddr).Port
	url := fmt.Sprintf("http://localhost:%d", port)

	files := http.FileServer(http.Dir(tmpDir))
	srv := &http.Server{
		Handler: http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			// WebGPU requires these headers; without them navigator.gpu is undefined
			w.Header().Set("Cross-Origin-Opener-Policy", "same-origin")
			w.Header().Set("Cross-Origin-Embedder-Policy", "require-corp")
			// some systems serve .wasm as application/octet-stream
			if filepath.Ext(r.URL.Path) == ".wasm" {
				w.Header().Set("Content-Type", "application/wasm")
			}
			files.ServeHTTP(w, r)
		}),
	}

	quit := make(chan os.Signal, 1)
	signal.Notify(quit, syscall.SIGINT, syscall.SIGTERM)

	go func() {
		if err := srv.Serve(listener); err != nil && err != http.ErrServerClosed {
			log.Fatalf("server: %v", err)
		}
	}()

	fmt.Printf("serving at %s\n", url)

	// give the server a moment before opening the browser
	time.Sleep(100 * time.Millisecond)
	openBrowser(url)

	<-quit
	fmt.Println("\nstopping.")
}

// repoRoot walks up from the current directory until it finds Cargo.toml.
func repoRoot() string {
	dir, _ := os.Getwd()
	for {
		if _, err := os.Stat(filepath.Join(dir, "Cargo.toml")); err == nil {
			return dir
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			log.Fatal("could not find Cargo.toml — run from anywhere inside the repo")
		}
		dir = parent
	}
}

func listTargets(root string) {
	data, err := os.ReadFile(filepath.Join(root, "Cargo.toml"))
	if err != nil {
		fmt.Fprintln(os.Stderr, "could not read Cargo.toml")
		return
	}
	var names []string
	for _, line := range strings.Split(string(data), "\n") {
		line = strings.TrimSpace(line)
		if strings.HasPrefix(line, "name") {
			// only collect names that follow an [[example]] section header
			val := strings.TrimSpace(strings.TrimPrefix(line, "name"))
			val = strings.TrimPrefix(val, "=")
			val = strings.TrimSpace(val)
			val = strings.Trim(val, `"`)
			names = append(names, val)
		}
	}
	// only [[example]] name entries, not [package] name — filter by checking
	// that each name has a matching examples/<name> directory
	fmt.Println("available targets:")
	found := false
	for _, n := range names {
		if _, err := os.Stat(filepath.Join(root, "examples", n)); err == nil {
			fmt.Printf("  %s\n", n)
			found = true
		}
	}
	if !found {
		fmt.Println("  (none found — run from the repo root)")
	}
	fmt.Println("\nusage: go run scripts/run_wasm.go <example_name>")
}

func openBrowser(url string) {
	var cmd *exec.Cmd
	switch runtime.GOOS {
	case "linux":
		cmd = exec.Command("xdg-open", url)
	case "darwin":
		cmd = exec.Command("open", url)
	case "windows":
		cmd = exec.Command("cmd", "/c", "start", "", url)
	default:
		fmt.Printf("open %s in your browser\n", url)
		return
	}
	if err := cmd.Start(); err != nil {
		fmt.Printf("open %s in your browser\n", url)
	}
}
