i do not need default_start_time. an event added with only date start (eg start:tomorrow) should be a full day event. an event with start date and time need default_duration. support two way: `calman add an event start:-0900 dur:1h` and `calman add an event start:-0900 end:-1100`

the `[ui]` section is wierd. if default_filter is for tui default filter, it should go under `[tui]`
show default icons for [icons.todo] and [icons.event] in config.default.toml
there should not be report.default.toml, merge that into config.default.toml
i like following format for colorscheme.rules
```toml
[colorscheme.rules]
completed = {fg="gray10"}
overdue = {inverse=true}
```

