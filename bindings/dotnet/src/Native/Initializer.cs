using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace Lunar.Native;

// runs automatically when the Lunar assembly is loaded — before any P/Invoke call.
// redirects [LibraryImport("lunar_ffi")] to the host executable's symbol table
// so game plugins don't need to know that lunar_ffi is statically linked into the host.
static class Initializer
{
    [ModuleInitializer]
    internal static void Init() =>
        NativeLibrary.SetDllImportResolver(
            typeof(Initializer).Assembly,
            static (name, _, _) => name == "lunar_ffi"
                ? NativeLibrary.GetMainProgramHandle()
                : IntPtr.Zero);
}
