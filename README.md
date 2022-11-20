# objtalk

[![Documentation](https://docs.rs/objtalk/badge.svg)](https://docs.rs/objtalk)
[![Automated Builds](https://github.com/objtalk/objtalk/actions/workflows/build.yaml/badge.svg)](https://github.com/objtalk/objtalk/actions/workflows/build.yaml)

objtalk is a lightweight realtime database that can be used for IoT projects. It can be used to store and query the state of devices in realtime and also to communicate between devices using events and rpc. Think mqtt but on steroids.

objtalk stores objects. Each object has a name, a value (json) and a timestamp that indicates when the object was last modified. Objects can be created, modified, removed and queried over an API. Queries can also watch for changes and return them in real time to the client.

The API can be accessed using multiple transports. The protocol is JSON-RPC-like and can be used over TCP or over a WebSocket. HTTP/REST can also be used for simple commands.

All objects stored in objtalk can also be viewed in a browser using the builtin admin panel.

## Installation

Install a [precompiled binary](https://github.com/objtalk/objtalk/releases) or use cargo:

```
$ cargo install objtalk
```

## Using the server

Create a config file, for example called `objtalk.toml`:

```toml
[storage]
backend = "sqlite"
sqlite.filename = "objtalk.db"

[[http]]
addr = "127.0.0.1:3000"
admin.enabled = true
#admin.asset-overrides = "admin"
#allow-origin = "*"

[[tcp]]
addr = "127.0.0.1:3001"
```

Start the server:

```sh
$ objtalk-server --config objtalk.toml
http transport listening on http://127.0.0.1:3000
tcp transport listening on 127.0.0.1:3001
```

Visit the admin panel at `http://127.0.0.1:3000`.

## Using the client

```sh
$ objtalk-cli -u http://127.0.0.1:3000 set foo 42
$ objtalk-cli -u http://127.0.0.1:3000 get foo
[
    {
        "name": "foo",
        "value": 42,
        "lastModified": "2021-05-07T17:53:29.066420Z"
    }
]
$ objtalk-cli -u http://127.0.0.1:3000 remove foo
```

## Using objtalk as a rust library

The objtalk crate provides the `objtalk-server` and `objtalk-cli` binaries, but you can also use it as a library to integrate objtalk into your rust project. Take a look at the [documentation](https://docs.rs/objtalk) for a list of all available methods. You can use the `server` and `client` feature flags to trim down the library.

## Libraries for other languages

- [JavaScript](https://www.npmjs.com/package/objtalk)

## Other integrations

- [Node-RED](https://flows.nodered.org/node/node-red-contrib-objtalk)

## API/Protocol

### Basics

#### set `name` `value`

`set` creates or replaces an object.

using objtalk-cli:

```
$ objtalk-cli set sensor '{"value":42}'
```

over http:

```
$ curl -X POST 127.0.0.1:3000/objects/sensor -d '{"temperature":20}'
```

over tcp or websocket:

```json
{
    "id": 1,
    "type": "set",
    "name": "sensor",
    "value": { "temperature": 20 }
}

{
    "requestId": 1,
    "result": {
        "success": true
    }
}
```

#### patch `name` `value`

`patch` creates or updates an object. If an object with the same name already exists the value is merged (non-deep).

using objtalk-cli:

```
$ objtalk-cli replace sensor '{"temperature":20}'
```

over http:

```
$ curl -X PATCH 127.0.0.1:3000/objects/sensor -d '{"temperature":20}'
```

over tcp or websocket:

```json
{
    "id": 1,
    "type": "patch",
    "name": "sensor",
    "value": {
        "temperature": 20
    }
}

{
    "requestId": 1,
    "result": {
        "success": true
    }
}
```

#### get `pattern`

`get` returns all objects where the name matches `pattern`.

Object names look like a file path and consist of multiple parts seperated by forward slashes. Each part can be either a string, a `+` or `*`. `+` matches anything until the next slash, `*` matches anything until the end of the string. Multiple sub-patterns can be combined using a comma and are logically OR-ed togeter.

Some examples:
- `*` matches all objects
- `device/lamp/+` matches `device/lamp/livingroom` and `device/lamp/bedroom`, but not `device/sensor/livingroom`
- `device/+/livingroom` matches `device/lamp/livingroom` and `device/sensor/livingroom`
- `device/*` matches `device/lamp/livingroom`, `device/lamp/bedroom` and `device/sensor/livingroom`
- `device/lamp/+,device/sensor/+` matches `device/lamp/livingroom`, `device/lamp/bedroom` and `device/sensor/livingroom`

using objtalk-cli:

```
$ objtalk-cli get '*'
```

over http:

```
$ curl '127.0.0.1:3000/query?pattern=*'
```

over tcp or websocket:

```json
{
    "id": 1,
    "type": "get",
    "pattern": "*"
}

{
    "requestId": 1,
    "result": {
        "objects": [
            {
                "name": "sensor",
                "value": { "temperature": 20 },
                "lastModified": "YYYY-MM-DDTHH:MM:SS.SSSSSSSSSZ"
            }
        ]
    }
}
```

#### query `pattern`

`query` returns all objects matching `pattern` and watches for changes. The initial response contains all objects that match `pattern`. When new objects with a matching name are created, changed or removed a `queryAdd` event, `queryChange` event or `queryRemove` event is emitted.

using objtalk-cli: unsupported

over http:

```
$ curl '127.0.0.1:3000/query?pattern=*' -H "Accept: text/event-stream"
```

over tcp or websocket:

```json
{
    "id": 1,
    "type": "query",
    "pattern": "*"
}

{
    "requestId": 1,
    "result": {
        "queryId": "01234567-89ab-cdef-0123-456789abcdef",
        "objects": [
            {
                "name": "sensor",
                "value": { "temperature": 20 },
                "lastModified": "YYYY-MM-DDTHH:MM:SS.SSSSSSSSSZ"
            }
        ]
    }
}

{
    "type": "queryChange",
    "queryId": "01234567-89ab-cdef-0123-456789abcdef",
    "object": {
        "name": "sensor",
        "value": { "temperature": 21 },
        "lastModified": "YYYY-MM-DDTHH:MM:SS.SSSSSSSSSZ"
    }
}

{
    "type": "queryAdd",
    "queryId": "01234567-89ab-cdef-0123-456789abcdef",
    "object": {
        "name": "foo",
        "value": { "bar": true },
        "lastModified": "YYYY-MM-DDTHH:MM:SS.SSSSSSSSSZ"
    }
}

{
    "type": "queryChange",
    "queryId": "01234567-89ab-cdef-0123-456789abcdef",
    "object": {
        "name": "foo",
        "value": { "bar": false },
        "lastModified": "YYYY-MM-DDTHH:MM:SS.SSSSSSSSSZ"
    }
}

{
    "type": "queryRemove",
    "queryId": "01234567-89ab-cdef-0123-456789abcdef",
    "object": {
        "name": "foo",
        "value": { "bar": false },
        "lastModified": "YYYY-MM-DDTHH:MM:SS.SSSSSSSSSZ"
    }
}
```

#### unsubscribe `queryId`

`unsubscribe` stops watching for changes and removes a query.

using objtalk-cli: unsupported

over http: implicit when the event-stream is closed

over tcp or websocket:

```json
{
    "id": 1,
    "type": "unsubscribe",
    "queryId": "01234567-89ab-cdef-0123-456789abcdef"
}
{
    "requestId": 1,
    "result": {
        "success": true
    }
}
```

#### remove `name`

`remove` removes an object. The return value indicates if an object with the name existed and was successfully removed.

using objtalk-cli:

```
$ objtalk-cli remove sensor
```

over http:

```
$ curl -X DELETE 127.0.0.1:3000/objects/sensor
```

over tcp or websocket:

```json
{
    "id": 1,
    "type": "remove",
    "name": "sensor"
}

{
    "requestId": 1,
    "result": {
        "existed": true
    }
}
```

### Events

Objects can also emit events. You can listen for events by creating a query.

#### emit `object` `event` `data`

`emit` publishes an event on an existing object.

using objtalk-cli: 

```
$ objtalk-cli emit gamepad buttonPress '{"button":"a"}'
```

over http:

```
$ curl -X POST 127.0.0.1:3000/events/gamepad -d '{"event":"buttonPress","data":{"button":"a"}}'
```

over tcp or websocket:

```json
{
    "id": 1,
    "type": "emit",
    "object": "gamepad",
    "event": "buttonPress",
    "data": { "button": "a" }
}

{
    "requestId": 1,
    "result": {
        "success": true
    }
}
```

Which produces the following event on all queries that match that object:

```json
{
    "type": "queryEvent",
    "queryId": "01234567-89ab-cdef-0123-456789abcdef",
    "object": "gamepad",
    "event": "buttonPress",
    "data": { "button": "a" }
}
```

### RPC

Methods can be called on objects. One client is connected to objtalk and listens for method calls, performs them and pushes the result to objtalk. Other clients can call these methods.

#### Calling methods

`invoke` calls a method on an object.

using objtalk-cli: 

```
$ objtalk-cli invoke device/lamp/livingroom setState '{"on":true}'
```

over http:

```
$ curl -X POST 127.0.0.1:3000/invoke/device/lamp/livingroom -d '{"method":"setState","args":{"on":true}}'
```

over tcp or websocket:

```json
{
    "id": 1,
    "type": "invoke",
    "object": "device/lamp/livingroom",
    "method": "setState",
    "args": { "on": true }
}

{
    "requestId": 1,
    "result": {
        "success": true
    }
}
```

#### Providing method calls

To provide rpc calls for an object a client ("provider") has to connect to objtalk and has to create a query with `provideRpc` set to true. Once another client ("consumer") tries to call a method on the object a `queryInvocation` event is emitted on the query. The provider can process the request and return a result to the consumer using the `invokeResult` command.

The whole flow looks like this:

```json
{
    "id": 1,
    "type": "query",
    "pattern": "device/lamp/livingroom",
    "provideRpc": true
}

{
    "requestId": 1,
    "result": {
        "queryId": "01234567-89ab-cdef-0123-456789abcdef",
        "objects": [
            {
                "name": "device/lamp/livingroom",
                "value": { "on": false },
                "lastModified": "YYYY-MM-DDTHH:MM:SS.SSSSSSSSSZ"
            }
        ]
    }
}

{
    "type": "queryInvocation",
    "queryId": "01234567-89ab-cdef-0123-456789abcdef",
    "invocationId": "fedcba98-7654-3210-fedc-ba9876543210",
    "object": "device/lamp/livingroom",
    "method": "setState",
    "args": { "on": true }
}

{
    "id": 2,
    "type": "invokeResult",
    "invocationId": "fedcba98-7654-3210-fedc-ba9876543210",
    "result": { "success": true }
}

{
    "requestId": 2,
    "result": {
        "success": true
    }
}
```
