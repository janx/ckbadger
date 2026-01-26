local counter = 0
local threads = {}

function setup(thread)
   thread:set("id", counter)
   table.insert(threads, thread)
   counter = counter + 1
end

function init(args)
   requests = 0
   responses = 0
   
   local endpoints = {
      "/api/v1/blocks?limit=10",
      "/api/v1/transactions?limit=10",
      "/api/v1/statistics/network",
      "/api/v1/cells/live?limit=10"
   }
   
   path = endpoints[(id % #endpoints) + 1]
end

function request()
   requests = requests + 1
   return wrk.format("GET", path)
end

function response(status, headers, body)
   responses = responses + 1
end

function done(summary, latency, requests)
   io.write("------------------------------\n")
   io.write("CKBEYE API Load Test Results\n")
   io.write("------------------------------\n")
   
   local duration = summary.duration / 1000000
   
   io.write(string.format("Duration:     %.2fs\n", duration))
   io.write(string.format("Requests:     %d\n", summary.requests))
   io.write(string.format("Throughput:   %.2f req/s\n", summary.requests / duration))
   io.write(string.format("Errors:       %d\n", summary.errors.connect + summary.errors.read + summary.errors.write + summary.errors.status + summary.errors.timeout))
   io.write("\n")
   io.write("Latency Distribution:\n")
   io.write(string.format("  50%%:  %.2fms\n", latency:percentile(50) / 1000))
   io.write(string.format("  75%%:  %.2fms\n", latency:percentile(75) / 1000))
   io.write(string.format("  90%%:  %.2fms\n", latency:percentile(90) / 1000))
   io.write(string.format("  95%%:  %.2fms\n", latency:percentile(95) / 1000))
   io.write(string.format("  99%%:  %.2fms\n", latency:percentile(99) / 1000))
   io.write(string.format("  Max:  %.2fms\n", latency.max / 1000))
   io.write("------------------------------\n")
end
