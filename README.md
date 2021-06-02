# objtalk

[![Documentation](https://docs.rs/objtalk/badge.svg)](https://docs.rs/objtalk)
[![Automated Builds](https://github.com/objtalk/objtalk/actions/workflows/build.yml/badge.svg)](https://github.com/objtalk/objtalk/actions/workflows/build.yml)

## Usage

### Server

Create a config file, for example `objtalk.toml`:

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

### Client

```sh
$ objtalk-cli set foo 42
$ objtalk-cli get foo
[
  {
    "name": "foo",
    "value": 42,
    "lastModified": "2021-05-07T17:53:29.066420Z"
  }
]
$ objtalk-cli remove foo
```

## Commands

### set `name` `value`

creates or replaces an object

### replace `name` `value`

creates or updates an object, new and existing object value is merged (non-deep)

### get `pattern`

returns all objects matching `pattern`

### query `pattern`

returns all objects matching `pattern` and watches for changes

### unsubscribe `queryId`

stops watching for changes

### remove `name`

removes an object

### emit `object` `event` `data`

emits a custom event on an existing object
