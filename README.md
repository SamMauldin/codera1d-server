# Codera1d Server

Codera1d server is the companion program to
[codera1d-client](https://github.com/SamMauldin/codera1d-client), allowing
collaborative code raiding in Rust.

## Usage

Build an image with the contained `Dockerfile`, add a volume to `/app/data/`,
set the environment variable `CODERA1D_API_KEY`, and expose port 8000 from the
container. Use the endpoint that the container is exposed to and the API Key
to build `codera1d-client`.
