//go:build !windows

package main

import (
	"errors"
	"os"
	"syscall"
)

func replaceFileAtomic(source, destination string) error {
	return os.Rename(source, destination)
}

func syncParentDirectory(path string) error {
	directory, err := os.Open(path)
	if err != nil {
		return err
	}
	defer directory.Close()
	err = directory.Sync()
	// 部分 FUSE/网络文件系统不支持目录 fsync；临时文件本身已经成功 fsync，保留兼容性。
	if errors.Is(err, syscall.EINVAL) || errors.Is(err, syscall.ENOTSUP) {
		return nil
	}
	return err
}
