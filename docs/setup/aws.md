# AWS setup

## Warnings

**NOTE that setting up Grapl _will_ incur AWS charges! This can amount to
hundreds of dollars a month based on the configuration.** This setup script is
designed for testing, and may include breaking changes in future versions,
increased charges in future versions, or may otherwise require manually working
with CloudFormation. If you need a way to set up Grapl in a stable, forwards
compatible manner, please get in contact with us directly.

## Preparation

### Local AWS credentials

See full instructions
[here](https://docs.aws.amazon.com/cli/latest/userguide/cli-chap-configure.html).

You should have a local file `~/.aws/credentials`, with an entry resembling this
format:

```
[my_profile]
aws_access_key_id=...
aws_secret_access_key=...
aws_session_token=...
```

You will need the **profile** to configure your account, if you haven't already:

`aws configure --profile "my_profile"`

If your profile's name is not "default", then note it down, as you will need to
include it as a parameter in later steps.

### Installing Dependencies

You'll need to have the following dependencies installed:

- Pulumi: https://www.pulumi.com/docs/get-started/install/
- AWS CLI:
  - your choice of the following:
    - `pip install awscli`
    - https://docs.aws.amazon.com/cli/latest/userguide/install-cliv2-docker.html
      - helpful alias:
        `alias aws='docker run --rm -it -v ~/.aws:/root/.aws -v $(pwd):/aws -e AWS_PROFILE amazon/aws-cli'`

### Clone Grapl Git repository

```bash
git clone https://github.com/grapl-security/grapl.git
cd grapl/
```

The remaining steps assume your working directory is the Grapl repository.

### Build deployment artifacts

Previously we supported uploading deployment artifacts (Docker images) directly
from your dev machine, but the current state of Grapl requires that the Docker
images be downloaded from Dockerhub or Cloudsmith. If you truly wish to upload
an image to Cloudsmith, try `bin/upload_image_to_cloudsmith.sh`

## Spin up infrastructure with Pulumi

(This section is actively under development, and as of Dec 2021 requires
infrastructure defined in the private repository
https://github.com/grapl-security/platform-infrastructure )

See
[pulumi/README.md](https://github.com/grapl-security/grapl/blob/main/pulumi/README.md)
for instructions to spin up infrastructure in AWS with Pulumi. Once you have
successfully deployed Grapl with Pulumi, return here and follow the instructions
in the following section to provision Grapl and run the tests.

## `graplctl`

We use the `graplctl` utility to manage Grapl in AWS.

### Installation

To install `graplctl` run the following command in the Grapl checkout root:

```bash
make graplctl
```

This will build the `graplctl` binary and install it in the `./bin/` directory.
You can familiarize yourself with `graplctl` by running

```bash
./bin/graplctl --help
```

#### Usage notes for setup

If your AWS profile is not named 'default', you will need to explicitly provide
it as a parameter:

- as a command line invocation parameter
- as an environmenal variable

#### Usage with Pulumi

Several commands will need references to things like S3 buckets or AWS log
groups. While you can pass these values directly, you can also pull them from a
Pulumi stack's outputs automatically.

To do this, you will need to export `GRAPLCTL_PULUMI_STACK` in your environment,
and then use the `./bin/graplctl-pulumi.sh` wrapper _instead_ of invoking
`graplctl` directly.

For further details, please read the documentation in that script.

## Testing

Follow the instructions in this section to deploy analyzers, upload test data,
and execute the end-to-end tests in AWS.

### Deploy analyzers

TBD

### Upload test data

TBD

### Logging in to the Grapl UI with the test user

You may use the test user to log into Grapl and interact with the UI. The test
username is the deployment name followed by `-grapl-test-user`. For example, if
your deployment was named `test-deployment`, your username would be
`test-deployment-grapl-test-user`.

To retrieve the password for your grapl deployment, navigate to "AWS Secrets
Manager" and click on "Secrets".

Click on the "Secret name" url that represents your deployment name followed by
`-TestUserPassword`. The link will bring you to the "secret details" screen.
Scroll down to the section labeled "Secret Value" and click the "Retrieve Secret
Value" button. The password for your deployment will appear under "Plaintext".

## DGraph operations

You can manage the DGraph cluster with the docker swarm tooling by logging into
one of the swarm managers with SSM. If you forget which instances are the swarm
managers, you can find them by running `graplctl swarm managers`. For your
convenience, `graplctl` also provides an `exec` command you can use to run a
bash command remotely on a swarm manager. For example, to list all the nodes in
the Dgraph swarm you can run something like the following:

```bash
bin/graplctl swarm exec --swarm-id my-swarm-id -- docker node ls
```

If you forget which `swarm-id` is associated with your Dgraph cluster, you may
list all the swarm IDs in your deployment by running `bin/graplctl swarm ls`.
