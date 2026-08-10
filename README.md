# plain-text-health

A set of tools to enable tracking of personal health+fitness data in plaintext
files.

# `.fitlog` File Format

`.fitlog` is the preferred extension for plain-text-health files. The files are,
obviously, plain text, and contain every configuration, definition, and log
entry that make up the entire fitness/health database managed by
`plain-text-health`.

## Comments

Anything after a `;` on the same line is ignored by the parser, except inside
quoted strings. This is used for writing comments.

Example:

```fitlog
; This is my first fitlog file!
metric weight lb ; Use pounds instead of kilograms
```

## Directives

`.fitlog` files are made up of directives. There are several types.

- **Pragma**
- **Declaration**
- **Entry**

### Pragmas

Instructions for the system to use while processing the data within `.fitlog`
files. Pragma directives always start with `!`.

#### Include

Use `!include "<file>"` to process the referenced file's directives as part of the
same database.

Since `plain-text-health` only accepts a single entrypoint filename, it's
important to use `!include` to include other files you may create to organize
your records.

Example:

```fitlog
!include "declarations.fitlog"
!include "2025.fitlog"
!include "2026.fitlog"
```

### Declarations

Provides information on a specific type of data that may be used by other
directives. There are multiple kinds of Declaration.

Every declaration has a `name`. `name` can be any combination of letters,
numbers, hyphens, and underscores, but must start with a letter or underscore.

#### Metric

`metric` is used to declare what types of measurements can be tracked in
Entries.

Syntax: `metric <name> <unit> [additive]`

`additive` is an optional flag that indicates whether entries on the same day
sum together rather than indicate a single measured value.

Example:

```fitlog
metric weight lb
metric bodyfat %
metric steps steps additive
metric calories kcal additive
```

##### Additive Metrics

Marking a metric `additive` is meant to indicate it will be used as part of a
"running total" for that day. This is for things like sleep, calories, steps,
etc. that may be measured multiple times in a day, but ultimately sum toward a
total day value.

If you define a metric entry on a date without a time, that is considered the
value for the entire day. 

```fitlog
2026-08-05 sleep 450 min
```

If you define multiple metric entries on the same date with distinct times,
those values are considered individual occurrences, and will sum together to
determine the total date value.

```fitlog
2026-08-05 08:30 sleep 300 min   ; last night
2026-08-05 15:30 sleep 90 min    ; afternoon nap — sums to 390
```

A metric defined within an activity belongs to that activity, not the day. For
example, steps defined in a hike activity will not count for the day's steps.
Those should be recorded in a separate entry for the day.


#### Compound Metrics

Multiple metrics may be combined into a shorthand "alias" to allow easily
recording multiple related values. At processing time the alias will be
expanded to the original set of metrics entries.

Syntax: `metric <name> = <metric_a> / <metric_b> [/ <metric_c> ...]`

Example:

```fitlog
metric bp_sys mmHg
metric bp_dia mmHg
metric bp = bp_sys / bp_dia
```

#### Activity

An activity, as opposed to a metric, is a single event that may have multiple
metrics, exercises, or metadata associated with it.

Syntax: `activity <name>`

Example:

```fitlog
activity hike
activity run
activity lift
```

#### Exercise

An exercise is data within an activity that specifies the precise type and
amount of work you did.

The exercise declaration has a name and 1 - 4 "slots" where you can choose to
record a combination of `load`, `reps`, `duration`, and/or `distance`.

Syntax: `exercise <name> <slot> [slot...]`

`[slot]` can be one of `load`, `reps`, `duration`, `distance`.

Units are not specified at declaration time. They are provided by each entry.

`load` is measured in mass, which can be one of `lb`, `kg`
`reps` is measured in count, which is unitless
`duration` is measured in time, which is one of `sec`, `min`, `hr`
`distance` is measured in distance, which is one of `m`, `km`, `mi`, `ft`, `yd`

Example:

```fitlog
exercise bench_press load reps
exercise lap_400m duration
```

### Entry

A directive that records a piece of data.

An Entry directive always starts with a date `YYYY-MM-DD` and, optionally, a
time `HH:MM`, followed by the data you are recording. The data can be a set of
metrics or an activity.

#### Tags

Tags can be used to loosely categorize entries. They can be added to the end of
an entry line, and must start with `#`.

Example:

```fitlog
2026-08-05 08:00 bp 120/75 #bpmeds
```

#### Metadata

Metadata is arbitrary string data that needs no declaration in advance. It can
be defined with any entry by defining `key: "value"` on indented lines beneath
the entry.

Note: Every metadata value must be a string surrounded in double quotes. Using
the double-quote character `"` in a metadata value is not currently supported.

Example:

```fitlog
2026-08-05 08:12 weight 165.0 lb
  note: "Hit my goal today!!"
  document: "assets/2026/2026-08-05-weight-loss-progress-picture.jpg"
```

#### Metrics Entry

A Metrics entry records one or more measurements for a particular day or time.

Syntax: `YYYY-MM-DD [HH:MM] <metric> <value> [unit] [, metric value unit...] [#tags...]`

Units are optional. If they are omitted, they will be inferred to match the
declaration. If they are specified, they must match the declaration.

Multiple metrics can be specified by separating them with a comma, or you can
put them on indented lines for clarity:

```fitlog
2026-08-05 08:12 weight 178.4 lb
  bodyfat 18.2 %
  bp 120/75
```

#### Activity Entry

Activity entries allow you to associate metrics, exercises, and metadata with
any activities you've declared (such as run, hike, lifting session, etc.)

Syntax:

```fitlog
<date> [time] <activity> ["description"] [#tags...]
  [exercises...]
  [metrics...]
  [metadata...]
```

Exercises are recorded by specifying the name of the exercise as specified in
the declaration, followed by a value for each slot (load, reps, duration,
and/or distance).

Commas can be used as a shorthand for repeated exercises with different values.
The name of the exercise is only stated once, followed by values for each entry
separated by commas.
Ex. `farmer_carry 90 lb 40 m, 70 lb 45 m`

Slash-listing may be used on up to one slot to repeat that exercise with the
list of slash-delimited values. The other slot values are copied to each
repeated record. Ex. `lap_400m 92/94/91 sec`

Metrics and metadata are written in the same syntax specified in "Metrics Entry"
above.

Example:

```fitlog
2026-08-05 lift "Thursday Night Gym Session" #upperbody
  bench_press 185 lb 5/5/4
  dumbbell_curl 45 lb 6/6/5
  dumbbell_shoulder_press 65 lb 7/6/6
  avg_hr 148 bpm
  document: "assets/2026/2026-08-05-my-huge-throbbing-muscles.jpg"
```

## Rough Architecture

plain-text-health reads in the complete contents of the entrypoint `.fitlog`
file, as well as any files referenced by include statements. It parses all of
the data contained within (while running basic consistency checks) and populates
an in-memory [Apache Arrow](https://arrow.apache.org/) data store with the
information.

Queries can then be executed on this data using
[DataFusion](https://datafusion.apache.org/).

Tools can then be built on top of this data foundation to provide dashboards,
insights, and import tools to pull health data from other applications and
services.

## Dev Environment

This project leverages [dev containers](https://containers.dev). You can use a
tool like [DevPod](https://devpod.sh) to spin up a container that has all the
dependencies you need to build and run the project.

```sh
devpod up . --ide codium # use whatever IDE you want
```

# Future Ideas
- Goal tracking