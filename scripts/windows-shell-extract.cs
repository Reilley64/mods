using System;
using System.Runtime.InteropServices;
using System.Text;
public static class ZipShellProbe {
    public sealed class Result {
        public string Stage;
        public int HResult;
        public string HResultHex;
        public string Error;
        public int? Win32Code;
        public int QueuedItems;
        public int? EnumerationHResult;
        public int? AbortedQueryHResult;
        public bool? Aborted;
    }
    [DllImport("ole32.dll", ExactSpelling=true)] static extern int CoCreateInstance(ref Guid clsid, IntPtr outer, uint context, ref Guid iid, out IntPtr obj);
    [DllImport("shell32.dll", CharSet=CharSet.Unicode, ExactSpelling=true)] static extern int SHCreateItemFromParsingName(string path, IntPtr bind, ref Guid iid, out IntPtr item);
    [DllImport("shell32.dll", CharSet=CharSet.Unicode, ExactSpelling=true)] static extern int SHParseDisplayName(string path, IntPtr bind, out IntPtr pidl, uint attributes, IntPtr resultAttributes);
    [DllImport("shell32.dll", ExactSpelling=true)] static extern int SHBindToObject(IntPtr parent, IntPtr pidl, IntPtr bind, ref Guid iid, out IntPtr folder);
    [DllImport("shell32.dll", ExactSpelling=true)] static extern int SHCreateItemWithParent(IntPtr parentPidl, IntPtr parent, IntPtr child, ref Guid iid, out IntPtr item);
    [DllImport("shlwapi.dll", CharSet=CharSet.Unicode, ExactSpelling=true)] static extern int StrRetToBufW(IntPtr strret, IntPtr pidl, StringBuilder buffer, uint count);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)] delegate int Flags(IntPtr self, uint flags);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)] delegate int EnumObjects(IntPtr self, IntPtr hwnd, uint flags, out IntPtr enumerator);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)] delegate int Next(IntPtr self, uint count, out IntPtr pidl, out uint fetched);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)] delegate int DisplayName(IntPtr self, IntPtr pidl, uint flags, IntPtr strret);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)] delegate int CopyItem(IntPtr self, IntPtr source, IntPtr target, IntPtr name, IntPtr sink);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)] delegate int Perform(IntPtr self);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)] delegate int Aborted(IntPtr self, out int aborted);
    static T Method<T>(IntPtr obj, int slot) where T : Delegate => Marshal.GetDelegateForFunctionPointer<T>(Marshal.ReadIntPtr(Marshal.ReadIntPtr(obj), slot * IntPtr.Size));
    static void Check(Result result, string stage, int hr) {
        result.Stage = stage;
        result.HResult = hr;
        if (hr < 0) Marshal.ThrowExceptionForHR(hr);
    }
    public static Result Extract(string archive, string destination) {
        var result = new Result();
        IntPtr op=IntPtr.Zero, target=IntPtr.Zero, full=IntPtr.Zero, folder=IntPtr.Zero, items=IntPtr.Zero, child=IntPtr.Zero, source=IntPtr.Zero;
        IntPtr strret = Marshal.AllocCoTaskMem(272);
        Guid clsid = new Guid("3AD05575-8857-4850-9277-11B85BDB8E09");
        Guid operation = new Guid("947AAB5F-0A5C-4C13-B4D6-4BF7836FC9F8");
        Guid shellItem = new Guid("43826D1E-E718-42EE-BC55-A1E261C37BFE");
        Guid shellFolder = new Guid("000214E6-0000-0000-C000-000000000046");
        try {
            Check(result,"CoCreateInstance",CoCreateInstance(ref clsid,IntPtr.Zero,23,ref operation,out op)); // CLSCTX_ALL
            Check(result,"SetOperationFlags",Method<Flags>(op,5)(op,0x0614)); // FOF_NO_UI
            Check(result,"SHCreateItemFromParsingName",SHCreateItemFromParsingName(destination,IntPtr.Zero,ref shellItem,out target));
            Check(result,"SHParseDisplayName",SHParseDisplayName(archive,IntPtr.Zero,out full,0,IntPtr.Zero));
            Check(result,"SHBindToObject",SHBindToObject(IntPtr.Zero,full,IntPtr.Zero,ref shellFolder,out folder));
            Check(result,"EnumObjects",Method<EnumObjects>(folder,4)(folder,IntPtr.Zero,0x60,out items));
            while (true) {
                uint fetched;
                int hr = Method<Next>(items,3)(items,1,out child,out fetched);
                result.EnumerationHResult = hr;
                // Archive.cpp ends enumeration on anything other than S_OK with one item.
                if (hr != 0 || fetched != 1) break;
                try {
                    Check(result,"GetDisplayNameOf",Method<DisplayName>(folder,11)(folder,child,0x8001,strret));
                    Check(result,"StrRetToBuf",StrRetToBufW(strret,child,new StringBuilder(260),260));
                    Check(result,"SHCreateItemWithParent",SHCreateItemWithParent(full,folder,child,ref shellItem,out source));
                    Check(result,"CopyItem",Method<CopyItem>(op,16)(op,source,target,IntPtr.Zero,IntPtr.Zero));
                    result.QueuedItems++;
                } finally {
                    if (source != IntPtr.Zero) { Marshal.Release(source); source=IntPtr.Zero; }
                    Marshal.FreeCoTaskMem(child); child=IntPtr.Zero;
                }
            }
            Check(result,"PerformOperations",Method<Perform>(op,21)(op));
            // Diagnostic only; not used by Archive.cpp, and does not change its result.
            int aborted;
            result.AbortedQueryHResult=Method<Aborted>(op,22)(op,out aborted);
            if (result.AbortedQueryHResult >= 0) result.Aborted=aborted != 0;
        } catch (Exception ex) {
            result.HResult=ex.HResult;
            result.Error=ex.Message;
        } finally {
            if (child != IntPtr.Zero) Marshal.FreeCoTaskMem(child);
            if (source != IntPtr.Zero) Marshal.Release(source);
            if (items != IntPtr.Zero) Marshal.Release(items);
            if (folder != IntPtr.Zero) Marshal.Release(folder);
            if (full != IntPtr.Zero) Marshal.FreeCoTaskMem(full);
            if (target != IntPtr.Zero) Marshal.Release(target);
            if (op != IntPtr.Zero) Marshal.Release(op);
            Marshal.FreeCoTaskMem(strret);
        }
        result.HResultHex="0x"+unchecked((uint)result.HResult).ToString("X8");
        if ((unchecked((uint)result.HResult) & 0x1FFF0000) == 0x00070000) result.Win32Code=result.HResult & 0xFFFF;
        return result;
    }
}
