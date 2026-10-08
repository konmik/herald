function Initialize-NativeShortcut {
    if ('herald.Shortcuts' -as [type]) { return }
    Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
using System.Runtime.InteropServices.ComTypes;

namespace herald {
    [ComImport, Guid("00021401-0000-0000-C000-000000000046")]
    internal class ShellLink {}

    [ComImport, Guid("000214F9-0000-0000-C000-000000000046"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    internal interface IShellLinkW {
        void GetPath([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder path, int count, IntPtr data, uint flags);
        void GetIDList(out IntPtr list);
        void SetIDList(IntPtr list);
        void GetDescription([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder description, int count);
        void SetDescription([MarshalAs(UnmanagedType.LPWStr)] string description);
        void GetWorkingDirectory([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder directory, int count);
        void SetWorkingDirectory([MarshalAs(UnmanagedType.LPWStr)] string directory);
        void GetArguments([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder arguments, int count);
        void SetArguments([MarshalAs(UnmanagedType.LPWStr)] string arguments);
        void GetHotkey(out short hotkey);
        void SetHotkey(short hotkey);
        void GetShowCmd(out int command);
        void SetShowCmd(int command);
        void GetIconLocation([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder path, int count, out int index);
        void SetIconLocation([MarshalAs(UnmanagedType.LPWStr)] string path, int index);
        void SetRelativePath([MarshalAs(UnmanagedType.LPWStr)] string path, uint reserved);
        void Resolve(IntPtr window, uint flags);
        void SetPath([MarshalAs(UnmanagedType.LPWStr)] string path);
    }

    public sealed class ShortcutInfo {
        public string TargetPath { get; set; }
        public string Arguments { get; set; }
        public string WorkingDirectory { get; set; }
    }

    public static class Shortcuts {
        public static ShortcutInfo Read(string path) {
            object instance = new ShellLink();
            try {
                ((IPersistFile)instance).Load(path, 0);
                var link = (IShellLinkW)instance;
                var target = new StringBuilder(32768);
                var arguments = new StringBuilder(32768);
                var directory = new StringBuilder(32768);
                link.GetPath(target, target.Capacity, IntPtr.Zero, 4);
                link.GetArguments(arguments, arguments.Capacity);
                link.GetWorkingDirectory(directory, directory.Capacity);
                return new ShortcutInfo { TargetPath = target.ToString(), Arguments = arguments.ToString(), WorkingDirectory = directory.ToString() };
            } finally { Marshal.FinalReleaseComObject(instance); }
        }

        public static void Write(string path, string target, string arguments, string directory) {
            object instance = new ShellLink();
            try {
                var link = (IShellLinkW)instance;
                link.SetPath(target);
                link.SetArguments(arguments);
                link.SetWorkingDirectory(directory);
                link.SetDescription("Herald settings");
                link.SetIconLocation(target, 0);
                ((IPersistFile)instance).Save(path, true);
            } finally { Marshal.FinalReleaseComObject(instance); }
        }
    }
}
'@
}

function Read-NativeShortcut {
    param([string]$Path)
    Initialize-NativeShortcut
    [herald.Shortcuts]::Read([IO.Path]::GetFullPath($Path))
}

function Write-NativeShortcut {
    param([string]$Path, [string]$TargetPath, [string]$Arguments, [string]$WorkingDirectory)
    Initialize-NativeShortcut
    [herald.Shortcuts]::Write([IO.Path]::GetFullPath($Path), [IO.Path]::GetFullPath($TargetPath), $Arguments, [IO.Path]::GetFullPath($WorkingDirectory))
}
