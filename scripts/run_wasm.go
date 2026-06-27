package main

import (
	"fmt"
	"io"
	"log"
	"net"
	"net/http"
	"os"
	"os/exec"
	"os/signal"
	"path/filepath"
	"runtime"
	"strings"
	"syscall"
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

	// wasm-opt shrinks and speeds up the bindgen output; soft-skip when absent
	if _, err := exec.LookPath("wasm-opt"); err == nil {
		bound := filepath.Join(tmpDir, name+"_bg.wasm")
		fmt.Println("running wasm-opt -O3...")
		opt := exec.Command("wasm-opt", "-O3", "--enable-simd", "--enable-bulk-memory", "-o", bound, bound)
		opt.Stdout = os.Stdout
		opt.Stderr = os.Stderr
		if err := opt.Run(); err != nil {
			log.Fatalf("wasm-opt failed: %v", err)
		}
	} else {
		fmt.Println("wasm-opt not found, skipping (install binaryen for smaller/faster wasm)")
	}

	// copy jolt physics sidecar if it has been built (optional, only needed for
	// physics examples). sidecar dist lives in the sibling jolt repo.
	sidecarDist := filepath.Join(filepath.Dir(root), "jolt", "jolt-rust", "sidecar", "dist")
	hasSidecar := fileExists(filepath.Join(sidecarDist, "jolt_sidecar.js")) &&
		fileExists(filepath.Join(sidecarDist, "jolt_sidecar.wasm"))
	if hasSidecar {
		for _, fname := range []string{"jolt_sidecar.js", "jolt_sidecar.wasm"} {
			if err := copyFile(filepath.Join(sidecarDist, fname), filepath.Join(tmpDir, fname)); err != nil {
				log.Fatalf("failed to copy sidecar file %s: %v", fname, err)
			}
		}
		fmt.Println("sidecar: jolt_sidecar.{js,wasm} included")
	}

	// build the module init block: load the jolt sidecar first when present so
	// window.__jolt is ready before the main wasm initializes.
	var moduleScript string
	if hasSidecar {
		moduleScript = fmt.Sprintf(`import createJoltModule from './jolt_sidecar.js';
window.__jolt = await createJoltModule();
import init from './%s.js';
await init();`, name)
	} else {
		moduleScript = fmt.Sprintf(`import init from './%s.js';
await init();`, name)
	}

	// minimal index.html: canvas fits the viewport while preserving aspect ratio.
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
canvas { display: block; width: min(100vw, calc(100vh * 1280 / 720)); aspect-ratio: 1280 / 720; }
</style>
</head>
<body>
<canvas id="lunar-canvas"></canvas>
<script>
(function() {
    var c = document.getElementById('lunar-canvas');
    function fit() {
        var dpr = window.devicePixelRatio || 1;
        var rect = c.getBoundingClientRect();
        var w = Math.round(rect.width * dpr);
        var h = Math.round(rect.height * dpr);
        if (w > 0 && h > 0 && (c.width !== w || c.height !== h)) {
            c.width = w; c.height = h;
        }
    }
    fit();
    new ResizeObserver(fit).observe(c);
    window.addEventListener('resize', fit);
})();
</script>
<script type="module">
%s
</script>
</body>
</html>`, name, moduleScript)

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
			log.Fatal("could not find Cargo.toml: run from anywhere inside the repo")
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
	// only [[example]] name entries, not [package] name: filter by checking
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
		fmt.Println("  (none found: run from the repo root)")
	}
	fmt.Println("\nusage: go run scripts/run_wasm.go <example_name>")
}

func fileExists(path string) bool {
	_, err := os.Stat(path)
	return err == nil
}

func copyFile(src, dst string) error {
	in, err := os.Open(src)
	if err != nil {
		return err
	}
	defer in.Close()
	out, err := os.Create(dst)
	if err != nil {
		return err
	}
	defer out.Close()
	_, err = io.Copy(out, in)
	return err
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
