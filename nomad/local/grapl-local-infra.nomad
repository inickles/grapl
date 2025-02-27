variable "image_tag" {
  type        = string
  description = "The tag for all container images we should deploy. This is ultimately set in the top-level Makefile."
}

variable "localstack_port" {
  type        = number
  description = "Port for Localstack"
  default     = 4566
}

variable "zookeeper_port" {
  type        = number
  description = "Port for Zookeeper"
  default     = 2181
}

locals {
  confluent_platform_version = "7.2.1"

  # This is the equivalent of `localhost` within a bridge network.
  # Useful for, for instance, talking to Zookeeper from Kafka without Consul Connect
  localhost_within_bridge = attr.unique.network.ip-address
  zookeeper_endpoint      = "${local.localhost_within_bridge}:${var.zookeeper_port}"

  # These Postgres connection data must match the `LocalPostgresInstance`s in
  # `pulumi/grapl/__main__.py`; sorry for the duplication :(
  database_descriptors = [
    {
      name = "plugin-registry-db",
      port = 5432,
    },
    {
      name       = "plugin-work-queue-db",
      port       = 5433,
      memory_max = 1024,
    },
    {
      name = "organization-management-db",
      port = 5434,
    },
    {
      name = "uid-allocator-db",
      port = 5435
    },
    {
      name = "event-source-db",
      port = 5436
    },
    {
      name = "graph-schema-manager-db",
      port = 5437
    },
  ]

  kafka_broker_descriptors = [
    {
      name             = "kafka0"
      broker_id        = 0
      broker_port      = 19092
      broker_port_host = 29092
      broker_port_task = 9092
      jmx_port         = 9101
    },
    {
      name             = "kafka1"
      broker_id        = 1
      broker_port      = 19093
      broker_port_host = 29093
      broker_port_task = 9093
      jmx_port         = 9102
    },
    {
      name             = "kafka2"
      broker_id        = 2
      broker_port      = 19094
      broker_port_host = 29094
      broker_port_task = 9094
      jmx_port         = 9103
    },
  ]
}


####################
# Jobspecs
####################
# NOTES:
# - Services in `grapl-core.nomad` should not try to service-discover
#   local-infra services via Consul Connect; use bridge+static.
#   This is because these services won't exist in prod.

# This job is to spin up infrastructure needed to run Grapl locally that we don't necessarily want to deploy in production (because AWS will manage it)
job "grapl-local-infra" {
  datacenters = ["dc1"]

  type = "service"

  group "localstack" {
    # Localstack will be available to Nomad Jobs (sans Consul Connect)
    # and the Host OS at localhost:4566
    network {
      mode = "bridge"
      port "localstack" {
        static = var.localstack_port
      }
    }

    task "localstack" {
      driver = "docker"

      config {
        image = "localstack/localstack-light:1.0.1"

        # Was running into this: https://github.com/localstack/localstack/issues/1349
        memory_hard_limit = 2048
        ports             = ["localstack"]
        privileged        = true
      }

      env {
        DEBUG     = 1
        EDGE_PORT = var.localstack_port
        SERVICES  = "dynamodb,ec2,iam,s3,secretsmanager,sns"

        # These are used by the health check below; "test" is the
        # default value for these credentials in Localstack.
        AWS_ACCESS_KEY_ID     = "test"
        AWS_SECRET_ACCESS_KEY = "test"
      }

      resources {
        cpu = 50
      }

      service {
        name = "localstack"
        check {
          type    = "script"
          name    = "check_s3_ls"
          command = "aws"
          args = [
            "--endpoint-url=http://localhost:${var.localstack_port}",
            "s3",
            "ls"
          ]
          interval = "10s"
          timeout  = "10s"

          check_restart {
            limit           = 2
            grace           = "30s"
            ignore_warnings = false
          }
        }
      }
    }
  }

  group "kafka" {
    network {
      mode = "bridge"

      dynamic "port" {
        for_each = local.kafka_broker_descriptors
        iterator = broker_descriptor

        labels = ["${broker_descriptor.value.name}-nomad"]

        content {
          static = broker_descriptor.value.broker_port
        }
      }

      dynamic "port" {
        for_each = local.kafka_broker_descriptors
        iterator = broker_descriptor

        labels = ["${broker_descriptor.value.name}-host"]

        content {
          static = broker_descriptor.value.broker_port_host
        }
      }
    }

    dynamic "task" {
      for_each = local.kafka_broker_descriptors
      iterator = broker_descriptor

      labels = [broker_descriptor.value.name]

      content {
        driver = "docker"

        config {
          image = "confluentinc/cp-kafka:${local.confluent_platform_version}"
          ports = [
            "${broker_descriptor.value.name}-nomad",
            "${broker_descriptor.value.name}-host"
          ]
        }

        resources {
          memory_max = 1256
          cpu        = 50
        }

        env {
          KAFKA_BROKER_ID         = broker_descriptor.value.broker_id
          KAFKA_ZOOKEEPER_CONNECT = local.zookeeper_endpoint

          # Some clients (like Pulumi) will need `host.docker.internal`
          # Some clients (like grapl-core services) will need localhost_within_bridge
          # We differentiate between which client it is based on which port we receive on.
          # So a receive on e.g. 29092 means HOST_OS
          KAFKA_ADVERTISED_LISTENERS = join(",", [
            "WITHIN_TASK://localhost:${broker_descriptor.value.broker_port_task}",
            "HOST_OS://host.docker.internal:${broker_descriptor.value.broker_port_host}",
            "OTHER_NOMADS://${local.localhost_within_bridge}:${broker_descriptor.value.broker_port}"
          ])
          KAFKA_AUTO_CREATE_TOPICS_ENABLE      = "false"
          KAFKA_LISTENER_SECURITY_PROTOCOL_MAP = "WITHIN_TASK:PLAINTEXT,HOST_OS:PLAINTEXT,OTHER_NOMADS:PLAINTEXT"
          KAFKA_INTER_BROKER_LISTENER_NAME     = "WITHIN_TASK"

          KAFKA_OFFSETS_TOPIC_REPLICATION_FACTOR         = 3
          KAFKA_TRANSACTION_STATE_LOG_MIN_ISR            = 2
          KAFKA_TRANSACTION_STATE_LOG_REPLICATION_FACTOR = 3
          KAFKA_GROUP_INITIAL_REBALANCE_DELAY_MS         = 0
          KAFKA_JMX_PORT                                 = broker_descriptor.value.jmx_port
          KAFKA_JMX_HOSTNAME                             = "localhost"
          KAFKA_LOG4J_ROOT_LOGLEVEL                      = "INFO"
        }

        service {
          name = broker_descriptor.value.name
          check {
            type    = "script"
            name    = "check_kafka"
            command = "nc"
            args = [
              "-v", # verbose
              "-z", # "zero I/O mode" - used for scanning
              "localhost",
              "${broker_descriptor.value.broker_port_task}"
            ]
            interval = "20s"
            timeout  = "10s"

            check_restart {
              limit           = 2
              grace           = "30s"
              ignore_warnings = false
            }
          }
        }
      }
    }
  }

  group "zookeeper" {
    network {
      mode = "bridge"
      port "zookeeper" {
        static = var.zookeeper_port
        to     = var.zookeeper_port
      }
    }

    task "zookeeper" {
      driver = "docker"

      config {
        image = "confluentinc/cp-zookeeper:${local.confluent_platform_version}"
        ports = ["zookeeper"] # may not be necessary
      }

      env {
        ZOOKEEPER_CLIENT_PORT = var.zookeeper_port
        ZOOKEEPER_TICK_TIME   = 2000
        KAFKA_OPTS            = "-Dzookeeper.4lw.commands.whitelist=ruok,dump"
      }

      resources {
        cpu = 50
      }

      service {
        name = "zookeeper"
        check {
          type    = "script"
          name    = "check_zookeeper"
          command = "/bin/bash"
          args = [
            "-o", "errexit", "-o", "nounset", "-o", "pipefail",
            "-c",
            "echo ruok | nc -w 2 localhost ${var.zookeeper_port} | grep imok || exit 2",
          ]
          interval = "20s"
          timeout  = "10s"

          check_restart {
            limit           = 2
            grace           = "30s"
            ignore_warnings = false
          }
        }
      }

    }
  }

  # Construct N groups for each entry in database_descriptors,
  # each one containing a Postgres task.
  dynamic "group" {
    for_each = local.database_descriptors
    iterator = db_desc

    labels = [db_desc.value.name]

    content {
      network {
        mode = "bridge"
        port "postgres" {
          static = db_desc.value.port
          to     = 5432 # postgres default
        }
      }

      # This is a hack so that the task name can be something dynamic.
      # (In this case, each task has the same name as the group.)
      # I do this because otherwise we'd have N logs called 'postgres.stdout'
      # It is for-each over a list with a single element: [db_desc].
      dynamic "task" {
        for_each = [db_desc.value]
        iterator = db_desc

        labels = [db_desc.value.name]

        content {
          driver = "docker"

          config {
            image = "postgres-ext:${var.image_tag}"
            ports = ["postgres"]

            # A jab at solving our Postgres memory woes, as mentioned on
            # https://hub.docker.com/_/postgres/
            # We don't see the error it's about, so we could be completely
            # barking up the wrong tree.
            shm_size = 268435456 # 256MB in bytes
          }

          env {
            POSTGRES_USER     = "postgres"
            POSTGRES_PASSWORD = "postgres"
          }

          service {
            name = db_desc.value.name

            check {
              type     = "script"
              name     = "check_postgres"
              command  = "pg_isready"
              args     = ["--username", "postgres"]
              interval = "20s"
              timeout  = "10s"

              check_restart {
                limit           = 2
                grace           = "30s"
                ignore_warnings = false
              }
            }
          }

          resources {
            memory_max = lookup(db_desc.value, "memory_max", 512) // (map, key, default) fyi
            cpu        = 50
          }
        }
      }
    }
  }

  group "scylla" {
    network {
      mode = "bridge"
      port "internal_node_rpc_1" {
        to = 7000
      }
      port "internal_node_rpc_2" {
        to = 7001
      }
      port "cql" {
        # Let devs connect via localhost:9042 from the host vm
        static = 9042
        to     = 9042
      }
      port "thrift" {
        to = 9160
      }
      port "rest" {
        to = 10000
      }
    }

    task "scylla" {
      driver = "docker"

      config {
        image = "scylladb-ext:${var.image_tag}"
        args = [
          # Set up scylla in single-node mode instead of in overprovisioned mode, ie DON'T use all available cpu/memory
          "--smp", "1"
        ]
        ports = ["internal_node_rpc_1", "internal_node_rpc_2", "cql", "thrift", "rest"]

        # Configure a data volume for scylla. See the "Configuring data volume for storage" section in
        # https://hub.docker.com/r/scylladb/scylla/
        mount {
          type     = "volume"
          target   = "/var/lib/scylla"
          source   = "scylla-data"
          readonly = false
          volume_options {
            # Upon initial creation of this volume, *do* copy in the current
            # contents in the Docker image.
            no_copy = false
            labels {
              maintainer = "Scylla"
            }
          }
        }
      }

      resources {
        cpu = 50
      }

      service {
        name = "scylla"

        check {
          type = "script"
          name = "nodestatus_check"
          # We use bin/bash so we can pipe to grep
          command  = "bin/bash"
          args     = ["nodetool", "status", "|", "grep", "'UN'"]
          interval = "30s"
          timeout  = "10s"

          check_restart {
            # Set readiness check since Scylla can take a while to boot up
            grace           = "1m"
            limit           = 3
            ignore_warnings = true
          }
        }
      }
    }
  }
}
