-- Register once; only original or earlier-interceptor messages can reach this callback.
local replies = 0
req:on_ws_message(function(ws)
    local message = ws.message
    if message.direction == "request" and message.type == "text" and message.body == "ping" then
        replies = replies + 1
        ws:drop()
        ws:send("response", json.encode({ reply = "pong", count = replies }), "text")
    end
end)
