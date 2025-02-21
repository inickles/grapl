#!/usr/bin/env bash

set -euo pipefail

# ISSUES:
# - How to do this selectively? I only really want to do this if we've
#   got real changes to build
# - Can this be decomposed in such a way that we can do it on a
#   per-container basis, rather than having to manage everything globally?

source .buildkite/scripts/lib/artifacts.sh
source .buildkite/scripts/lib/version.sh

RUST_VERSION="$(./build-support/rust_version.sh)"
export RUST_VERSION
PYTHON_VERSION="$(./build-support/python_version.sh)"
export PYTHON_VERSION

# While we have Docker Compose files present, we have to explicitly
# declare we're using an HCL file (compose YAML files are used
# preferentially, in the absence of explicit overrides).
#
# The name of this variable is our own; there doesn't appear to be an
# official one to specify such a file.
readonly BUILDX_BAKE_FILE="docker-bake.hcl"

# The target in our ${BAKE_HCL_FILE} file that defines all the images
# for us to build and push to Cloudsmith. If you want to add a new
# image, make sure it's part of this target.
readonly BUILDX_TARGET="cloudsmith-images"

# This triggers release builds to be made; see ${BAKE_HCL_FILE} for more
IMAGE_TAG="$(timestamp_and_sha_version)"
export IMAGE_TAG

# NOTE: We could theoretically collapse these two commands into a
# single Makefile target, but I have opted to structure them like this
# while we have to do the "check if the image is new" logic to keep
# all the buildx file introspection (and thus build-target awareness)
# localised here.
make build-image-prerequisites

# https://github.com/grapl-security/issue-tracker/issues/931
# Try the `buildx --push` first; if retried on Buildkite, do the slower
# manual approach.

if [[ "${BUILDKITE_RETRY_COUNT}" -eq 0 ]]; then
    echo "--- Build & Pushing all ${IMAGE_TAG} images"
    if ! docker buildx bake --file="${BUILDX_BAKE_FILE}" \
        --progress "plain" --push "${BUILDX_TARGET}"; then
        echo "buildx bake --push failed; Buildkite should auto-retry"
        # Special status for "buildx --push failed"
        exit 42
    fi
else
    echo "--- Building all ${IMAGE_TAG} images"
    # Build targets
    docker buildx bake --file="${BUILDX_BAKE_FILE}" --progress "plain" "${BUILDX_TARGET}"

    echo "--- Pushing all ${IMAGE_TAG} images"
    # Upload each image in serial, not parallel
    mapfile -t fully_qualified_images < <(
        docker buildx bake \
            --file="${BUILDX_BAKE_FILE}" \
            --print "${BUILDX_TARGET}" |
            jq --raw-output '.target | to_entries | .[] | .value.tags[0]'
    )
    for fq_image in "${fully_qualified_images[@]}"; do
        echo "--- Pushing ${fq_image}"
        docker push "${fq_image}"
    done
fi

########################################################################
# Determine whether or not each image is "new"
#
# Until we can inspect the source of our Rust services to determine if
# they need a new image, we will build the images, but then query the
# upstream registry to see if that content exists there already or
# not.
#
# This does require us to build the images first (which wastes a bit
# of time). It also requires us to push the images to our `raw`
# repository first in order to obtain the image's sha256 checksum
# (there doesn't appear to be a way to do this purely locally,
# amazingly enough!). It also requires this script to be aware that
# we'll ultimately be promoting to our `testing` repository. These are
# all unfortunate. However, it will make deployments quicker and more
# responsive, since services should churn less (they'll only restart
# when a new image is available, rather than for every single
# deployment).

readonly sleep_seconds=80
echo "--- :sleeping::sob: Sleeping for ${sleep_seconds} seconds to give CDNs time to update"
# Lately, we've seen failures where images aren't showing up when we
# run `docker manifest inspect` (see below), but rerunning the job
# succeeds. For the time being, we'll add a sleep to account for that.
#
# Yes, I hate it, too.
#
# As of 8/31/2022 this is still needed and in fact the sleep seconds needs to be increased :sob:
sleep "${sleep_seconds}"

# This is the list of services that actually have different images.
new_services=()

# This is where our images will ultimately be promoted to. It is the
# registry we'll need to query to see if an image with the same
# content already exists.
readonly UPSTREAM_REGISTRY="docker.cloudsmith.io/grapl/testing"

# It seems you can only get the sha256 sum of an image after pushing
# it to a registry. Fun.
#
# Returns a string like `sha256:deadbeef....`
function sha256_of_image() {
    docker manifest inspect --verbose "${1}" | jq --raw-output '.Descriptor.digest'
}

# Returns 0 if it is present; 1 if not.
#
# We'll go ahead and allow the output to go to our logs; that will
# help debugging.
function image_present_upstream() {
    docker manifest inspect "${1}"
}

echo "--- :cloudsmith::sleuth_or_spy: Checking upstream repository to determine what to promote"
# Generate a TSV of "${SERVICE}\t${TAG}" for each image we're
# pushing to Cloudsmith
#
# NOTE: This assumes that we have at least one tag for each image
# (which should be true!) and that this tag is for our Cloudsmith
# "raw" repository (which should also be true!)
while IFS=$'\t' read -r service tag; do
    echo "--- :cloudsmith: Checking '${service}:${IMAGE_TAG}' in 'grapl/testing'"
    sha256="$(sha256_of_image "${tag}")"
    echo "${tag} has identifier '${sha256}'"
    upstream_sha256_identifier="${UPSTREAM_REGISTRY}/${service}@${sha256}"

    echo "Checking the existence of '${upstream_sha256_identifier}'"
    if ! image_present_upstream "${upstream_sha256_identifier}"; then
        echo "Image not present upstream; adding '${service}' to the list of images to promote"
        new_services+=("${service}")
    else
        echo "Image already found upstream; nothing else to be done"
    fi
done < <(docker buildx bake --file="${BUILDX_BAKE_FILE}" "${BUILDX_TARGET}" --print |
    jq --raw-output '.target | to_entries[] | [.key, .value.tags[0]] | @tsv')

# Now that we've filtered out things that already exist upstream, we
# only need to care about the new stuff.
artifact_json "${IMAGE_TAG}" "${new_services[@]}" > "$(artifacts_file_for containers)"
