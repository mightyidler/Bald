using System;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;

public static class InstallerIconProbe {
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] struct ShellInfo {
        public IntPtr Icon; public int Index; public uint Attributes;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst=260)] public string DisplayName;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst=80)] public string TypeName;
    }
    [StructLayout(LayoutKind.Sequential)] struct BitmapInfo {
        public uint Size; public int Width, Height; public ushort Planes, BitCount;
        public uint Compression, ImageSize; public int XPels, YPels; public uint Used, Important;
    }
    [DllImport("shell32.dll", CharSet=CharSet.Unicode)] static extern IntPtr SHGetFileInfo(string path, uint attributes, out ShellInfo info, uint size, uint flags);
    [DllImport("user32.dll")] static extern bool DestroyIcon(IntPtr icon);
    [DllImport("user32.dll")] static extern bool DrawIconEx(IntPtr dc, int x, int y, IntPtr icon, int width, int height, uint step, IntPtr brush, uint flags);
    [DllImport("gdi32.dll")] static extern IntPtr CreateCompatibleDC(IntPtr dc);
    [DllImport("gdi32.dll")] static extern IntPtr CreateDIBSection(IntPtr dc, ref BitmapInfo info, uint usage, out IntPtr bits, IntPtr section, uint offset);
    [DllImport("gdi32.dll")] static extern IntPtr SelectObject(IntPtr dc, IntPtr item);
    [DllImport("gdi32.dll")] static extern bool DeleteObject(IntPtr item);
    [DllImport("gdi32.dll")] static extern bool DeleteDC(IntPtr dc);
    [DllImport("gdi32.dll")] static extern bool GdiFlush();

    // Ask the Shell for the shortcut's rendered icon, without launching its target.
    public static string ShellIconHash(string path) {
        ShellInfo info;
        if(SHGetFileInfo(path,0,out info,(uint)Marshal.SizeOf(typeof(ShellInfo)),0x100)==IntPtr.Zero || info.Icon==IntPtr.Zero)
            throw new InvalidDataException("Shell could not obtain icon: "+path);
        IntPtr dc=CreateCompatibleDC(IntPtr.Zero), bitmap=IntPtr.Zero, previous=IntPtr.Zero;
        try {
            var header=new BitmapInfo {Size=40,Width=32,Height=-32,Planes=1,BitCount=32};
            IntPtr bits;
            bitmap=CreateDIBSection(dc,ref header,0,out bits,IntPtr.Zero,0);
            if(dc==IntPtr.Zero || bitmap==IntPtr.Zero) throw new InvalidDataException("Could not render Shell icon");
            previous=SelectObject(dc,bitmap);
            byte[] pixels=new byte[32*32*4];
            Marshal.Copy(pixels,0,bits,pixels.Length);
            if(!DrawIconEx(dc,0,0,info.Icon,32,32,0,IntPtr.Zero,3)) throw new InvalidDataException("Could not draw Shell icon");
            GdiFlush();
            Marshal.Copy(bits,pixels,0,pixels.Length);
            using(var hash=System.Security.Cryptography.SHA256.Create()) return BitConverter.ToString(hash.ComputeHash(pixels));
        } finally {
            if(previous!=IntPtr.Zero) SelectObject(dc,previous);
            if(bitmap!=IntPtr.Zero) DeleteObject(bitmap);
            if(dc!=IntPtr.Zero) DeleteDC(dc);
            DestroyIcon(info.Icon);
        }
    }
    delegate bool ResourceName(IntPtr module, IntPtr kind, IntPtr name, IntPtr context);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern IntPtr LoadLibraryEx(string file, IntPtr reserved, uint flags);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool EnumResourceNames(IntPtr module, IntPtr kind, ResourceName callback, IntPtr context);
    [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr FindResource(IntPtr module, IntPtr name, IntPtr kind);
    [DllImport("kernel32.dll")] static extern uint SizeofResource(IntPtr module, IntPtr resource);
    [DllImport("kernel32.dll")] static extern IntPtr LoadResource(IntPtr module, IntPtr resource);
    [DllImport("kernel32.dll")] static extern IntPtr LockResource(IntPtr resource);
    [DllImport("kernel32.dll")] static extern bool FreeLibrary(IntPtr module);

    static byte[] Read(IntPtr module, IntPtr name, int kind) {
        IntPtr resource=FindResource(module,name,new IntPtr(kind));
        if(resource==IntPtr.Zero) throw new InvalidDataException("Missing icon resource");
        byte[] data=new byte[SizeofResource(module,resource)];
        Marshal.Copy(LockResource(LoadResource(module,resource)),data,0,data.Length);
        return data;
    }

    // Load as data only. Verification must never launch the installed application.
    public static int Verify(string executable, string icon) {
        byte[] source=File.ReadAllBytes(icon);
        if(source.Length<6 || BitConverter.ToUInt32(source,0)!=0x00010000) throw new InvalidDataException("Invalid ICO header");
        int count=BitConverter.ToUInt16(source,4);
        if(count==0 || source.Length<6+count*16) throw new InvalidDataException("Empty ICO");
        IntPtr module=LoadLibraryEx(executable,IntPtr.Zero,0x22);
        if(module==IntPtr.Zero) throw new System.ComponentModel.Win32Exception();
        try {
            byte[] group=null;
            ResourceName callback=(m,t,n,c)=>{group=Read(m,n,14);return false;};
            EnumResourceNames(module,new IntPtr(14),callback,IntPtr.Zero);
            GC.KeepAlive(callback);
            if(group==null || group.Length<6+count*14 || BitConverter.ToUInt16(group,4)!=count) throw new InvalidDataException("EXE icon sizes differ from ICO");
            for(int i=0;i<count;i++) {
                int entry=6+i*16, resourceEntry=6+i*14;
                int length=checked((int)BitConverter.ToUInt32(source,entry+8));
                int offset=checked((int)BitConverter.ToUInt32(source,entry+12));
                if(offset<0 || length<1 || offset>source.Length-length) throw new InvalidDataException("Invalid ICO image offset");
                byte[] expected=new byte[length];Array.Copy(source,offset,expected,0,length);
                byte[] actual=Read(module,new IntPtr(BitConverter.ToUInt16(group,resourceEntry+12)),3);
                if(!expected.SequenceEqual(actual)) throw new InvalidDataException("EXE icon pixels differ from ICO at size "+source[entry]);
            }
            return count;
        } finally { FreeLibrary(module); }
    }
}
