# plain-text-health

A set of tools to enable tracking of personal health+fitness data in plaintext
files.

## Rough Architecture

plain-text-health reads in the complete contents of the entrypoint `.pth` file,
as well as any files referenced by include statements. It parses all of the data
contained within (while running basic consistency checks) and populates an
in-memory [Apache Arrow](https://arrow.apache.org/) data store with the
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