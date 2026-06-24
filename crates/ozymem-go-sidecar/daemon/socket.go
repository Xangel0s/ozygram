package daemon

import (
	"bufio"
	"encoding/json"
	"fmt"
	"log"
	"net"
	"runtime"
	"os"
	"strings"
	"sync"
)

// SocketCommand represents a command received from the CLI or MCP server.
type SocketCommand struct {
	Cmd   string `json:"cmd"`   // "wake" | "sleep" | "status" | "register" | "list" | "ping"
	Path  string `json:"path"`  // Filesystem path of the project
	Name  string `json:"name"`  // Optional project name
	Force bool   `json:"force"` // Force operation (e.g., re-register)
}

// SocketResponse is sent back to the caller after processing a command.
type SocketResponse struct {
	Ok     bool   `json:"ok"`
	Status string `json:"status,omitempty"` // "ACTIVE" | "SLEEPING" | "REGISTERED" | "PONG"
	Scale  string `json:"scale,omitempty"`
	Error  string `json:"error,omitempty"`
	Data   string `json:"data,omitempty"` // JSON-encoded extra data (e.g., project list)
}

// SocketServer listens on a named pipe (Windows) or Unix socket and dispatches
// commands to the daemon's handler function.
type SocketServer struct {
	listener net.Listener
	handler  CommandHandler
	mu       sync.Mutex
	closed   bool
}

// CommandHandler is the function signature for processing socket commands.
// The daemon implements this to handle wake/sleep/status/register commands.
type CommandHandler func(cmd SocketCommand) SocketResponse

// NewSocketServer creates and starts listening on the platform-specific socket.
func NewSocketServer(handler CommandHandler) (*SocketServer, error) {
	socketPath := SocketPath()
	var listener net.Listener
	var err error

	if runtime.GOOS == "windows" {
		// On Windows, use a named pipe via winio or fallback to TCP localhost
		// For simplicity and cross-platform compatibility, use TCP on a fixed port
		listener, err = net.Listen("tcp", "127.0.0.1:17399")
		if err != nil {
			return nil, fmt.Errorf("failed to listen on 127.0.0.1:17399: %w", err)
		}
		log.Printf("[SOCKET] Listening on tcp://127.0.0.1:17399 (Windows mode)")
	} else {
		// On Unix, use a domain socket
		_ = os.Remove(socketPath) // Clean up stale socket
		listener, err = net.Listen("unix", socketPath)
		if err != nil {
			return nil, fmt.Errorf("failed to listen on %s: %w", socketPath, err)
		}
		log.Printf("[SOCKET] Listening on unix://%s", socketPath)
	}

	server := &SocketServer{
		listener: listener,
		handler:  handler,
	}

	go server.acceptLoop()
	return server, nil
}

// Close shuts down the socket server.
func (s *SocketServer) Close() error {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.closed = true
	return s.listener.Close()
}

// acceptLoop continuously accepts connections and handles them.
func (s *SocketServer) acceptLoop() {
	for {
		conn, err := s.listener.Accept()
		if err != nil {
			s.mu.Lock()
			closed := s.closed
			s.mu.Unlock()
			if closed {
				return
			}
			log.Printf("[SOCKET] Accept error: %v", err)
			continue
		}
		go s.handleConnection(conn)
	}
}

// handleConnection reads one JSON command per line, dispatches it,
// and writes the JSON response back.
func (s *SocketServer) handleConnection(conn net.Conn) {
	defer conn.Close()

	scanner := bufio.NewScanner(conn)
	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if line == "" {
			continue
		}

		var cmd SocketCommand
		if err := json.Unmarshal([]byte(line), &cmd); err != nil {
			resp := SocketResponse{Ok: false, Error: fmt.Sprintf("invalid JSON: %v", err)}
			writeResponse(conn, resp)
			continue
		}

		log.Printf("[SOCKET] Received command: %s (path=%s, name=%s)", cmd.Cmd, cmd.Path, cmd.Name)
		resp := s.handler(cmd)
		writeResponse(conn, resp)
	}
}

// writeResponse serializes and writes a SocketResponse to the connection.
func writeResponse(conn net.Conn, resp SocketResponse) {
	data, _ := json.Marshal(resp)
	data = append(data, '\n')
	conn.Write(data)
}

// --------------------------------------------------------------------------
// Client-side: Send commands to the daemon
// --------------------------------------------------------------------------

// SendCommand connects to the daemon socket and sends a single command.
// Returns the daemon's response.
func SendCommand(cmd SocketCommand) (*SocketResponse, error) {
	var conn net.Conn
	var err error

	if runtime.GOOS == "windows" {
		conn, err = net.Dial("tcp", "127.0.0.1:17399")
	} else {
		conn, err = net.Dial("unix", SocketPath())
	}
	if err != nil {
		return nil, fmt.Errorf("cannot connect to daemon: %w", err)
	}
	defer conn.Close()

	// Send command
	data, _ := json.Marshal(cmd)
	data = append(data, '\n')
	if _, err := conn.Write(data); err != nil {
		return nil, fmt.Errorf("failed to write command: %w", err)
	}

	// Read response
	scanner := bufio.NewScanner(conn)
	if !scanner.Scan() {
		return nil, fmt.Errorf("no response from daemon")
	}

	var resp SocketResponse
	if err := json.Unmarshal(scanner.Bytes(), &resp); err != nil {
		return nil, fmt.Errorf("invalid response from daemon: %w", err)
	}

	return &resp, nil
}
