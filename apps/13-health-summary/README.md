# 13 - health-summary

Shows the range and latest values of recorded heart rate, body weight and
blood pressure. It also counts observations by category and lists recorded
active conditions. Optional data adds current medications and ECG lengths.

## Build and run

```sh
make run APP=13-health-summary
make run APP=13-health-summary OPTIONAL=off
make run APP=13-health-summary FIXTURES=fixtures/no-call
make run APP=13-health-summary FIXTURES=fixtures/unanswered
```

The default root supplies all eight grants. Both smaller roots lack the
required health paths, so the local runner stops before processing there.
The [fixture guide](../../fixtures/README.md#health-records) describes the
invented records, and [health data paths](../../docs/03-data-paths.md#health-records)
documents their formats and absence rules.
