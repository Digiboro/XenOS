/**
 * test-driver-c - Test boot driver in C for XenOS
 * 
 * Simple driver that creates a device \Device\XenTestC
 * and handles basic IRP_MJ_CREATE/CLOSE/READ/WRITE
 *
 * Build: ./docker-build.sh (Docker + Wine + WDK 7600.16385.1)
 */

#include <ntddk.h>

/* Device name */
static WCHAR DeviceNameBuffer[] = L"\\Device\\XenTestC";
static UNICODE_STRING DeviceName;

/* Global device object pointer for DriverUnload */
static PDEVICE_OBJECT g_DeviceObject = NULL;

/* Internal buffer for read/write operations */
#define DEVICE_BUFFER_SIZE 256
static UCHAR g_DeviceBuffer[DEVICE_BUFFER_SIZE];
static ULONG g_DeviceBufferLength = 0;

/* ========================================================================== */
/* Dispatch routines                                                          */
/* ========================================================================== */

static NTSTATUS NTAPI DispatchCreate(
    IN PDEVICE_OBJECT DeviceObject,
    IN PIRP Irp)
{
    (void)DeviceObject;
    
    Irp->IoStatus.Status = STATUS_SUCCESS;
    Irp->IoStatus.Information = 0;
    IoCompleteRequest(Irp, IO_NO_INCREMENT);
    
    return STATUS_SUCCESS;
}

static NTSTATUS NTAPI DispatchClose(
    IN PDEVICE_OBJECT DeviceObject,
    IN PIRP Irp)
{
    (void)DeviceObject;
    
    Irp->IoStatus.Status = STATUS_SUCCESS;
    Irp->IoStatus.Information = 0;
    IoCompleteRequest(Irp, IO_NO_INCREMENT);
    
    return STATUS_SUCCESS;
}

/* IRP_MJ_CLEANUP handler - called when last handle is closed */
static NTSTATUS NTAPI DispatchCleanup(
    IN PDEVICE_OBJECT DeviceObject,
    IN PIRP Irp)
{
    (void)DeviceObject;
    
    /* For test driver, just complete successfully */
    /* Real drivers should cancel pending IRPs here */
    Irp->IoStatus.Status = STATUS_SUCCESS;
    Irp->IoStatus.Information = 0;
    IoCompleteRequest(Irp, IO_NO_INCREMENT);
    
    return STATUS_SUCCESS;
}

static NTSTATUS NTAPI DispatchRead(
    IN PDEVICE_OBJECT DeviceObject,
    IN PIRP Irp)
{
    PIO_STACK_LOCATION irpSp;
    ULONG length;
    ULONG toRead;
    ULONG i;
    PVOID buffer;
    
    (void)DeviceObject;
    
    irpSp = IoGetCurrentIrpStackLocation(Irp);
    length = irpSp->Parameters.Read.Length;
    buffer = Irp->AssociatedIrp.SystemBuffer;
    
    /* Calculate how much we can read */
    toRead = (ULONG)g_DeviceBufferLength;
    if (toRead > length) {
        toRead = length;
    }
    
    /* Copy data to user buffer */
    if (toRead > 0 && buffer != NULL) {
        for (i = 0; i < toRead; i++) {
            ((PUCHAR)buffer)[i] = g_DeviceBuffer[i];
        }
    }
    
    Irp->IoStatus.Status = STATUS_SUCCESS;
    Irp->IoStatus.Information = toRead;
    IoCompleteRequest(Irp, IO_NO_INCREMENT);
    
    return STATUS_SUCCESS;
}

static NTSTATUS NTAPI DispatchWrite(
    IN PDEVICE_OBJECT DeviceObject,
    IN PIRP Irp)
{
    PIO_STACK_LOCATION irpSp;
    ULONG length;
    ULONG toWrite;
    ULONG i;
    PVOID buffer;
    
    (void)DeviceObject;
    
    irpSp = IoGetCurrentIrpStackLocation(Irp);
    length = irpSp->Parameters.Write.Length;
    buffer = Irp->AssociatedIrp.SystemBuffer;
    
    /* Calculate how much we can write */
    toWrite = length;
    if (toWrite > DEVICE_BUFFER_SIZE) {
        toWrite = DEVICE_BUFFER_SIZE;
    }
    
    /* Copy data from user buffer */
    if (toWrite > 0 && buffer != NULL) {
        for (i = 0; i < toWrite; i++) {
            g_DeviceBuffer[i] = ((PUCHAR)buffer)[i];
        }
        g_DeviceBufferLength = toWrite;
    }
    
    Irp->IoStatus.Status = STATUS_SUCCESS;
    Irp->IoStatus.Information = toWrite;
    IoCompleteRequest(Irp, IO_NO_INCREMENT);
    
    return STATUS_SUCCESS;
}

/* ========================================================================== */
/* DriverUnload                                                               */
/* ========================================================================== */

static VOID NTAPI DriverUnload(
    IN PDRIVER_OBJECT DriverObject)
{
    (void)DriverObject;
    
    if (g_DeviceObject != NULL) {
        IoDeleteDevice(g_DeviceObject);
        g_DeviceObject = NULL;
    }
}

/* ========================================================================== */
/* DriverEntry - Driver entry point                                           */
/* ========================================================================== */

NTSTATUS NTAPI DriverEntry(
    IN PDRIVER_OBJECT DriverObject,
    IN PUNICODE_STRING RegistryPath)
{
    NTSTATUS status;
    
    (void)RegistryPath;
    
    /* Initialize device name string manually (RtlInitUnicodeString not available) */
    DeviceName.Buffer = DeviceNameBuffer;
    DeviceName.Length = sizeof(DeviceNameBuffer) - sizeof(WCHAR);
    DeviceName.MaximumLength = sizeof(DeviceNameBuffer);
    
    /* Create device object */
    status = IoCreateDevice(
        DriverObject,
        0,                          /* DeviceExtensionSize */
        &DeviceName,
        FILE_DEVICE_UNKNOWN,
        0,                          /* DeviceCharacteristics */
        FALSE,                      /* Exclusive */
        &g_DeviceObject
    );
    
    if (!NT_SUCCESS(status)) {
        return status;
    }
    
    /* Set buffered I/O flag */
    g_DeviceObject->Flags |= DO_BUFFERED_IO;
    
    /* Set dispatch routines */
    DriverObject->MajorFunction[IRP_MJ_CREATE] = DispatchCreate;
    DriverObject->MajorFunction[IRP_MJ_CLOSE] = DispatchClose;
    DriverObject->MajorFunction[IRP_MJ_CLEANUP] = DispatchCleanup;
    DriverObject->MajorFunction[IRP_MJ_READ] = DispatchRead;
    DriverObject->MajorFunction[IRP_MJ_WRITE] = DispatchWrite;
    DriverObject->DriverUnload = DriverUnload;
    
    return STATUS_SUCCESS;
}

