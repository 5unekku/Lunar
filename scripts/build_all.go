package main

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

// target describes one cross-compilation target.
type target struct {
	triple string
	// tool is the cargo subcommand to use: "build", "zigbuild", or "xwin".
	tool string
	// ext is the binary file extension ("" for unix, ".exe" for windows).
	ext string
	// sdk is a human-readable note about a required external SDK ("" = none).
	// when non-empty the build is attempted but failures are reported softly.
	sdk string
}

var allTargets = []target{
	// ── linux (glibc) ────────────────────────────────────────────────────
	{"x86_64-unknown-linux-gnu", "zigbuild", "", ""},
	{"aarch64-unknown-linux-gnu", "zigbuild", "", ""},
	{"i686-unknown-linux-gnu", "zigbuild", "", ""},
	{"armv7-unknown-linux-gnueabihf", "zigbuild", "", ""},

	// ── linux (musl — fully static) ──────────────────────────────────────
	{"x86_64-unknown-linux-musl", "zigbuild", "", ""},
	{"aarch64-unknown-linux-musl", "zigbuild", "", ""},
	{"i686-unknown-linux-musl", "zigbuild", "", ""},
	{"armv7-unknown-linux-musleabihf", "zigbuild", "", ""},

	// ── windows (gnu — no MSVC SDK required) ─────────────────────────────
	{"x86_64-pc-windows-gnu", "zigbuild", ".exe", ""},
	{"i686-pc-windows-gnu", "zigbuild", ".exe", ""},
	// aarch64 uses gnullvm (LLVM-mingw) since there is no aarch64-pc-windows-gnu
	{"aarch64-pc-windows-gnullvm", "zigbuild", ".exe", ""},

	// ── macos (requires macOS SDK via osxcross) ───────────────────────────
	{"x86_64-apple-darwin", "zigbuild", "", "macos-sdk (set SDKROOT or install osxcross)"},
	{"aarch64-apple-darwin", "zigbuild", "", "macos-sdk (set SDKROOT or install osxcross)"},
}

func main() {
	patchWindowsToolchains()
	patchMacosToolchains()
	patchSdl3Compat()

	root := repoRoot()
	release := false
	only := ""
	example := ""

	for i := 1; i < len(os.Args); i++ {
		switch os.Args[i] {
		case "--release":
			release = true
		case "--target":
			i++
			if i < len(os.Args) {
				only = os.Args[i]
			}
		case "--example":
			i++
			if i < len(os.Args) {
				example = os.Args[i]
			}
		case "--help", "-h":
			printUsage()
			os.Exit(0)
		}
	}

	examples := collectExamples(root)
	if len(examples) == 0 {
		fmt.Fprintln(os.Stderr, "no examples found in Cargo.toml")
		os.Exit(1)
	}
	if example != "" {
		examples = []string{example}
	}

	targets := allTargets
	if only != "" {
		targets = nil
		for _, t := range allTargets {
			if t.triple == only {
				targets = append(targets, t)
			}
		}
		if len(targets) == 0 {
			fmt.Fprintf(os.Stderr, "unknown target %q — valid targets:\n", only)
			for _, t := range allTargets {
				fmt.Fprintf(os.Stderr, "  %s\n", t.triple)
			}
			os.Exit(1)
		}
	}

	checkTools(targets)

	profile := "debug"
	if release {
		profile = "release"
	}

	type result struct {
		triple  string
		example string
		ok      bool
		note    string
	}
	var results []result

	for _, t := range targets {
		for _, ex := range examples {
			label := fmt.Sprintf("%s / %s", t.triple, ex)
			fmt.Printf("\n── building %s ──\n", label)
			if t.sdk != "" {
				fmt.Printf("   note: %s\n", t.sdk)
			}

			outDir := filepath.Join(root, "dist", t.triple)
			if err := os.MkdirAll(outDir, 0755); err != nil {
				fmt.Fprintf(os.Stderr, "   failed to create dist dir: %v\n", err)
				results = append(results, result{t.triple, ex, false, err.Error()})
				continue
			}

			args := buildArgs(t, ex, release)
			cmd := exec.Command("cargo", args...)
			cmd.Dir = root
			// merge stderr into stdout so everything appears in a redirected log
			cmd.Stdout = os.Stdout
			cmd.Stderr = os.Stdout
			if t.sdk != "" {
				// cross-compiling for macos: clear pkg-config host library dirs and
				// disable SDL HIDAPI so cmake does not try to validate libusb as a
				// .dylib (find_library still returns the linux .so despite root path
				// restrictions when CMAKE_FIND_ROOT_PATH is empty)
				cmd.Env = append(os.Environ(),
					"PKG_CONFIG_LIBDIR=",
					"PKG_CONFIG_PATH=",
					"CARGO_FEATURE_NO_SDL_HIDAPI=1",
				)
			}

			err := cmd.Run()
			if err != nil {
				msg := err.Error()
				if t.sdk != "" {
					msg = fmt.Sprintf("build failed (sdk required: %s)", t.sdk)
				}
				fmt.Printf("   FAILED: %s\n", msg)
				results = append(results, result{t.triple, ex, false, msg})
				continue
			}

			// copy binary to dist/
			src := filepath.Join(root, "target", t.triple, profile, "examples", ex+t.ext)
			dst := filepath.Join(outDir, ex+t.ext)
			if copyErr := copyFile(src, dst); copyErr != nil {
				fmt.Printf("   built but could not copy to dist: %v\n", copyErr)
				results = append(results, result{t.triple, ex, false, copyErr.Error()})
			} else {
				fmt.Printf("   → dist/%s/%s%s\n", t.triple, ex, t.ext)
				results = append(results, result{t.triple, ex, true, ""})
			}
		}
	}

	// summary
	fmt.Printf("\n%s\n", strings.Repeat("─", 60))
	ok, failed := 0, 0
	for _, r := range results {
		if r.ok {
			ok++
			fmt.Printf("  ✓  %s / %s\n", r.triple, r.example)
		} else {
			failed++
			fmt.Printf("  ✗  %s / %s  (%s)\n", r.triple, r.example, r.note)
		}
	}
	fmt.Printf("%s\n%d succeeded, %d failed\n", strings.Repeat("─", 60), ok, failed)

	if failed > 0 {
		os.Exit(1)
	}
}

func buildArgs(t target, example string, release bool) []string {
	var args []string
	switch t.tool {
	case "zigbuild":
		args = []string{"zigbuild"}
	default:
		args = []string{"build"}
	}
	args = append(args, "--target", t.triple, "--example", example)
	if release {
		args = append(args, "--release")
	}
	return args
}

func checkTools(targets []target) {
	needed := map[string]bool{}
	for _, t := range targets {
		if t.tool != "build" {
			needed[t.tool] = true
		}
	}
	for tool := range needed {
		subcommand := tool
		if _, err := exec.LookPath("cargo"); err == nil {
			// check if cargo subcommand exists by running it with --help
			cmd := exec.Command("cargo", subcommand, "--help")
			cmd.Stdout = nil
			cmd.Stderr = nil
			if cmd.Run() != nil {
				fmt.Printf("warning: cargo-%s not found — targets using it will fail\n", tool)
				fmt.Printf("  install: cargo install cargo-%s\n", tool)
			}
		}
	}
}

func collectExamples(root string) []string {
	data, err := os.ReadFile(filepath.Join(root, "Cargo.toml"))
	if err != nil {
		return nil
	}
	var names []string
	for _, line := range strings.Split(string(data), "\n") {
		line = strings.TrimSpace(line)
		if strings.HasPrefix(line, "name") {
			val := strings.TrimSpace(strings.TrimPrefix(line, "name"))
			val = strings.TrimPrefix(val, "=")
			val = strings.Trim(strings.TrimSpace(val), `"`)
			if _, err := os.Stat(filepath.Join(root, "examples", val)); err == nil {
				names = append(names, val)
			}
		}
	}
	return names
}

func repoRoot() string {
	dir, _ := os.Getwd()
	for {
		if _, err := os.Stat(filepath.Join(dir, "Cargo.toml")); err == nil {
			return dir
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			fmt.Fprintln(os.Stderr, "could not find Cargo.toml — run from inside the repo")
			os.Exit(1)
		}
		dir = parent
	}
}

func copyFile(src, dst string) error {
	data, err := os.ReadFile(src)
	if err != nil {
		return err
	}
	return os.WriteFile(dst, data, 0755)
}

// patchWindowsToolchains creates a 'windres' symlink in ~/.local/bin pointing to llvm-windres.
// cargo-zigbuild regenerates cmake toolchain files on every run so patching them is not viable.
// The cmake-generated Makefile calls bare 'windres'; a symlink in PATH is the reliable fix.
func patchWindowsToolchains() {
	llvmWindres, err := exec.LookPath("llvm-windres")
	if err != nil {
		fmt.Println("warning: llvm-windres not found — Windows builds may fail on .rc files")
		return
	}
	home, _ := os.UserHomeDir()
	dest := filepath.Join(home, ".local", "bin", "windres")
	if _, err := os.Lstat(dest); err == nil {
		return // already exists
	}
	if err := os.Symlink(llvmWindres, dest); err == nil {
		fmt.Printf("created windres symlink: %s -> %s\n", dest, llvmWindres)
	}
}

// patchSdl3Compat fixes a 32-bit compile bug in sdl3-0.18.4 in the cargo registry.
// XlibWindowHandle::new(window as u64) fails on 32-bit where c_ulong is u32.
// 'as _' lets Rust infer the correct type for each platform.
func patchSdl3Compat() {
	home, _ := os.UserHomeDir()
	pattern := filepath.Join(home, ".cargo", "registry", "src", "index.crates.io-*", "sdl3-0.18.4", "src", "sdl3", "raw_window_handle.rs")
	matches, _ := filepath.Glob(pattern)
	for _, path := range matches {
		data, err := os.ReadFile(path)
		if err != nil {
			continue
		}
		content := string(data)
		fixed := strings.ReplaceAll(content, "window as u64)", "window as _)")
		if fixed == content {
			continue
		}
		if err := os.WriteFile(path, []byte(fixed), 0644); err == nil {
			fmt.Println("patched sdl3 raw_window_handle.rs for 32-bit targets")
		}
	}
}

// patchMacosToolchains adds CMAKE_SYSROOT and CMAKE_FIND_ROOT_PATH to zigbuild's macOS
// cmake toolchain files, sourced from $SDKROOT at cmake configure time.
// Without a sysroot, CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY has no effect and cmake's
// find_library falls back to host paths, picking up Linux .so files for the macOS target.
func patchMacosToolchains() {
	home, _ := os.UserHomeDir()
	pattern := filepath.Join(home, ".cache", "cargo-zigbuild", "*", "wrappers", "*", "cmake", "*apple*toolchain.cmake")
	matches, _ := filepath.Glob(pattern)
	for _, path := range matches {
		data, err := os.ReadFile(path)
		if err != nil || strings.Contains(string(data), "CMAKE_SYSROOT") {
			continue
		}
		addition := `
if(DEFINED ENV{SDKROOT})
  set(CMAKE_SYSROOT "$ENV{SDKROOT}")
  set(CMAKE_FIND_ROOT_PATH "$ENV{SDKROOT}")
endif()
`
		patched := strings.TrimRight(string(data), "\n") + addition
		if err := os.WriteFile(path, []byte(patched), 0644); err == nil {
			fmt.Printf("patched cmake sysroot: %s\n", filepath.Base(path))
		}
	}
}

func printUsage() {
	fmt.Println(`usage: go run scripts/build_all.go [options]

options:
  --release            build in release mode (default: debug)
  --target <triple>    build only the specified target triple
  --example <name>     build only the specified example (default: all)
  --help               show this message

targets:`)
	for _, t := range allTargets {
		sdk := ""
		if t.sdk != "" {
			sdk = "  [sdk required]"
		}
		fmt.Printf("  %-40s %s%s\n", t.triple, t.tool, sdk)
	}
}
