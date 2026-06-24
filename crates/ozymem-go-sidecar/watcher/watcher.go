package watcher

import (
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"strings"

	"github.com/fsnotify/fsnotify"
)

// Watcher monitors a directory recursively for file write/create events.
type Watcher struct {
	fsWatcher *fsnotify.Watcher
	Events    chan string
	Errors    chan error
	ignores   map[string]bool
}

// NewWatcher creates a new Watcher instance with a list of ignored folder names.
func NewWatcher(ignoreDirs []string) (*Watcher, error) {
	fsw, err := fsnotify.NewWatcher()
	if err != nil {
		return nil, err
	}

	ignores := make(map[string]bool)
	for _, dir := range ignoreDirs {
		ignores[strings.ToLower(dir)] = true
	}

	return &Watcher{
		fsWatcher: fsw,
		Events:    make(chan string, 100),
		Errors:    make(chan error, 10),
		ignores:   ignores,
	}, nil
}

// Start recursively registers directory paths and starts the event listening loop.
func (w *Watcher) Start(rootPath string) error {
	absRoot, err := filepath.Abs(rootPath)
	if err != nil {
		return fmt.Errorf("failed to get absolute path: %w", err)
	}

	// Recursively add directories to the watcher
	err = filepath.WalkDir(absRoot, func(path string, d fs.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		if d.IsDir() {
			name := strings.ToLower(d.Name())
			if w.ignores[name] || strings.HasPrefix(name, ".") {
				return filepath.SkipDir
			}
			err := w.fsWatcher.Add(path)
			if err != nil {
				return fmt.Errorf("failed to watch path %s: %w", path, err)
			}
		}
		return nil
	})

	if err != nil {
		w.fsWatcher.Close()
		return err
	}

	go w.listenLoop()
	return nil
}

// Close releases the fsnotify resources.
func (w *Watcher) Close() error {
	return w.fsWatcher.Close()
}

func (w *Watcher) listenLoop() {
	for {
		select {
		case event, ok := <-w.fsWatcher.Events:
			if !ok {
				close(w.Events)
				return
			}
			// Focus on Write and Create operations
			if event.Has(fsnotify.Write) || event.Has(fsnotify.Create) {
				// Verify if it's a file and not in ignored directories
				info, err := os.Stat(event.Name)
				if err == nil && !info.IsDir() {
					w.Events <- event.Name
				}
			}
		case err, ok := <-w.fsWatcher.Errors:
			if !ok {
				close(w.Errors)
				return
			}
			w.Errors <- err
		}
	}
}
