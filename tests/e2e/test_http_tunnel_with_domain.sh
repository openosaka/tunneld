#!/bin/bash
set -x

root_dir=$(git rev-parse --show-toplevel)
cur_dir=$root_dir/tests/e2e
source $cur_dir/util.sh

# Function to clean up processes
cleanup() {
  echo "Cleaning up..."
  kill -SIGINT $client_pid
  kill -SIGINT $client_pid2
  kill -SIGINT $server_pid
  kill $http_server_pid1
  kill $http_server_pid2
}

# Trap EXIT signal to ensure cleanup
trap cleanup EXIT

# Start the tunnel server
exec $root_dir/target/debug/castled &
server_pid=$!

wait_port 6610

# Start the tunnel client
run_client http 12347 --domain "foo.com"
client_pid=$!

sleep 1

# test can't bind to the same subdomain
run_client_block http 6666 --domain "foo.com"
error_code=$?
echo $error_code
if [[ $error_code -eq 0 ]]; then
	error "Test failed: Expected non-zero error code, got $error_code"
	exit 1
fi

# Start the nc TCP server
exec $cur_dir/.bin/ping --port 12347 &
http_server_pid1=$!

wait_port 12347

response=$(curl -s -H "Host: foo.com" http://localhost:6611/ping?query=castled)
if [[ $response != "pong=castled" ]]; then
	error "Test failed: Response is not pong=castled, is $response"
	exit 1
fi

# kill the client and register again
kill -SIGINT $client_pid
sleep 1
run_client http 12347 --domain foo.com
client_pid=$!

run_client http 12346 --domain bar.com
client_pid2=$!
exec $cur_dir/.bin/ping --port 12346 &
http_server_pid2=$!

wait_port 12346

response=$(curl -s -H "Host: foo.com" http://localhost:6611/ping?query=server1)
if [[ $response != "pong=server1" ]]; then
	error "Test failed: Response is not pong=server1, is $response"
	exit 2
fi

response=$(curl -s -H "Host: bar.com" http://localhost:6611/ping?query=server2)
if [[ $response != "pong=server2" ]]; then
	error "Test failed: Response is not pong=server1, is $response"
	exit 3
fi
