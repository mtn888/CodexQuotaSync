//go:build windows

package main

import (
	"syscall"
	"unsafe"
)

const (
	moveFileReplaceExisting = 0x1
	moveFileWriteThrough    = 0x8
)

var (
	kernel32       = syscall.NewLazyDLL("kernel32.dll")
	moveFileExProc = kernel32.NewProc("MoveFileExW")
)

func replaceFileAtomic(source, destination string) error {
	sourcePointer, err := syscall.UTF16PtrFromString(source)
	if err != nil {
		return err
	}
	destinationPointer, err := syscall.UTF16PtrFromString(destination)
	if err != nil {
		return err
	}
	result, _, callErr := moveFileExProc.Call(
		uintptr(unsafe.Pointer(sourcePointer)),
		uintptr(unsafe.Pointer(destinationPointer)),
		moveFileReplaceExisting|moveFileWriteThrough,
	)
	if result == 0 {
		if callErr != nil && callErr != syscall.Errno(0) {
			return callErr
		}
		return syscall.EINVAL
	}
	return nil
}

func syncParentDirectory(string) error {
	// MoveFileExW 的 MOVEFILE_WRITE_THROUGH 会等待文件替换落盘；Windows 不支持对目录调用 File.Sync。
	return nil
}
