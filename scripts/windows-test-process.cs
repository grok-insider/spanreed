using System;
using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Security;
using System.Text;
using System.Threading;

public sealed class WindowsTestProcess : IDisposable
{
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct StartupInfo
    {
        public uint Size;
        public string Reserved, Desktop, Title;
        public uint X, Y, XSize, YSize, XCountChars, YCountChars, FillAttribute, Flags;
        public ushort ShowWindow, ReservedSize;
        public IntPtr ReservedData, StandardInput, StandardOutput, StandardError;
    }
    [StructLayout(LayoutKind.Sequential)]
    private struct ProcessInformation { public IntPtr Process, Thread; public uint ProcessId, ThreadId; }
    [StructLayout(LayoutKind.Sequential)]
    private struct BasicLimit
    {
        public long PerProcessUserTimeLimit, PerJobUserTimeLimit;
        public uint LimitFlags;
        public UIntPtr MinimumWorkingSetSize, MaximumWorkingSetSize;
        public uint ActiveProcessLimit;
        public UIntPtr Affinity;
        public uint PriorityClass, SchedulingClass;
    }
    [StructLayout(LayoutKind.Sequential)]
    private struct IoCounters { public ulong ReadOperationCount, WriteOperationCount, OtherOperationCount, ReadTransferCount, WriteTransferCount, OtherTransferCount; }
    [StructLayout(LayoutKind.Sequential)]
    private struct ExtendedLimit
    {
        public BasicLimit Basic;
        public IoCounters Io;
        public UIntPtr ProcessMemoryLimit, JobMemoryLimit, PeakProcessMemoryUsed, PeakJobMemoryUsed;
    }
    [StructLayout(LayoutKind.Sequential)]
    private struct BasicAccounting
    {
        public long TotalUserTime, TotalKernelTime, ThisPeriodUserTime, ThisPeriodKernelTime;
        public uint TotalPageFaultCount, TotalProcesses, ActiveProcesses, TotalTerminatedProcesses;
    }
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr CreateJobObjectW(IntPtr attributes, string name);
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool SetInformationJobObject(IntPtr job, int infoClass, ref ExtendedLimit info, uint length);
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool TerminateJobObject(IntPtr job, uint exitCode);
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool QueryInformationJobObject(IntPtr job, int infoClass, out BasicAccounting info, uint length, IntPtr returnedLength);
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern uint ResumeThread(IntPtr thread);
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool TerminateProcess(IntPtr process, uint exitCode);
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool GetExitCodeProcess(IntPtr process, out uint exitCode);
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool CloseHandle(IntPtr handle);
    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool CreateProcessWithLogonW(string username, string domain, IntPtr password, uint logonFlags, string application, StringBuilder commandLine, uint creationFlags, IntPtr environment, string currentDirectory, ref StartupInfo startup, out ProcessInformation information);

    private IntPtr job, process;
    public uint ProcessId { get; private set; }

    public static WindowsTestProcess Start(string username, SecureString password, string application, string commandLine, string environment, string directory)
    {
        var child = new WindowsTestProcess();
        IntPtr passwordBuffer = IntPtr.Zero, environmentBuffer = IntPtr.Zero;
        ProcessInformation information = new ProcessInformation();
        try
        {
            child.job = CreateJobObjectW(IntPtr.Zero, null);
            if (child.job == IntPtr.Zero) throw new Win32Exception(Marshal.GetLastWin32Error());
            var limits = new ExtendedLimit();
            limits.Basic.LimitFlags = 0x2000;
            if (!SetInformationJobObject(child.job, 9, ref limits, (uint)Marshal.SizeOf(typeof(ExtendedLimit)))) throw new Win32Exception(Marshal.GetLastWin32Error());
            passwordBuffer = Marshal.SecureStringToGlobalAllocUnicode(password);
            environmentBuffer = Marshal.StringToHGlobalUni(environment);
            var startup = new StartupInfo();
            startup.Size = (uint)Marshal.SizeOf(typeof(StartupInfo));
            if (!CreateProcessWithLogonW(username, Environment.MachineName, passwordBuffer, 1, application, new StringBuilder(commandLine), 0x08000404, environmentBuffer, directory, ref startup, out information)) throw new Win32Exception(Marshal.GetLastWin32Error());
            child.process = information.Process;
            child.ProcessId = information.ProcessId;
            if (!AssignProcessToJobObject(child.job, child.process)) throw new Win32Exception(Marshal.GetLastWin32Error());
            if (ResumeThread(information.Thread) != 1) throw new InvalidOperationException("Test process was not resumed exactly once");
            return child;
        }
        catch
        {
            if (child.process != IntPtr.Zero) { TerminateProcess(child.process, 1); WaitForSingleObject(child.process, 5000); }
            child.Dispose();
            throw;
        }
        finally
        {
            if (information.Thread != IntPtr.Zero) CloseHandle(information.Thread);
            if (passwordBuffer != IntPtr.Zero) Marshal.ZeroFreeGlobalAllocUnicode(passwordBuffer);
            if (environmentBuffer != IntPtr.Zero) Marshal.FreeHGlobal(environmentBuffer);
        }
    }
    public bool Wait(uint milliseconds)
    {
        uint result = WaitForSingleObject(process, milliseconds);
        if (result == 0) return true;
        if (result == 258) return false;
        throw new Win32Exception(Marshal.GetLastWin32Error());
    }
    public uint ExitCode
    {
        get { uint result; if (!GetExitCodeProcess(process, out result)) throw new Win32Exception(Marshal.GetLastWin32Error()); return result; }
    }
    public void Dispose()
    {
        try
        {
            if (job != IntPtr.Zero)
            {
                if (!TerminateJobObject(job, 1)) throw new Win32Exception(Marshal.GetLastWin32Error());
                var watch = Stopwatch.StartNew();
                for (;;)
                {
                    BasicAccounting accounting;
                    if (!QueryInformationJobObject(job, 1, out accounting, (uint)Marshal.SizeOf(typeof(BasicAccounting)), IntPtr.Zero)) throw new Win32Exception(Marshal.GetLastWin32Error());
                    if (accounting.ActiveProcesses == 0) break;
                    if (watch.ElapsedMilliseconds >= 5000) throw new TimeoutException("Windows test Job did not become empty");
                    Thread.Sleep(50);
                }
            }
        }
        finally
        {
            if (job != IntPtr.Zero) { CloseHandle(job); job = IntPtr.Zero; }
            if (process != IntPtr.Zero) { WaitForSingleObject(process, 5000); CloseHandle(process); process = IntPtr.Zero; }
        }
    }
}
