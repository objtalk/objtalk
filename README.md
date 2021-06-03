# objtalk

[![Documentation](https://docs.rs/objtalk/badge.svg)](https://docs.rs/objtalk)
[![Automated Builds](https://github.com/objtalk/objtalk/actions/workflows/build.yaml/badge.svg)](https://github.com/objtalk/objtalk/actions/workflows/build.yaml)

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

## Commands

### set `name` `value`

creates or replaces an object

using objtalk-cli:

```
$ objtalk-cli set sensor {"value":42}
```

over http:

```
$ curl -X POST 127.0.0.1:3000/objects/sensor -d {"value":42}
```

### replace `name` `value`

creates or updates an object, new and existing object value is merged (non-deep)

using objtalk-cli: 

```
$ objtalk-cli replace sensor {"value":42}
```

over http:

```
$ curl -X PATCH 127.0.0.1:3000/objects/sensor -d {"value":42}
```

### get `pattern`

returns all objects matching `pattern`

using objtalk-cli:

```
$ objtalk-cli get '*'
```

over http:

```
$ curl 127.0.0.1:3000/query?pattern=* -d {"value":42}
```

### query `pattern`

returns all objects matching `pattern` and watches for changes

using objtalk-cli: unsupported

over http:

```
$ curl 127.0.0.1:3000/query?pattern=* -H "Accept: text/event-stream"
```

### unsubscribe `queryId`

stops watching for changes

using objtalk-cli: unsupported

over http: unsupported

### remove `name`

removes an object

using objtalk-cli:

```
$ objtalk-cli remove sensor
```

over http:

```
$ curl -X DELETE 127.0.0.1:3000/objects/sensor
```

### emit `object` `event` `data`

emits a custom event on an existing object

using objtalk-cli: 

```
$ objtalk-cli emit gamepad buttonPress {"button":"a"}
```

over http:

```
$ curl -X POST 127.0.0.1:3000/events/gamepad -d {"event":"buttonPress","data":{"button":"a"}}
```
