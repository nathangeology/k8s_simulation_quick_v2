# EKS Analytics Schema

## Node

### Metadata Source

DB : `resources_snapshot`
table : `node_v1`

### Schema Description

|Column Name	|Type	|Assumed Type	|Source	|Description	|Example	|Ref Link	|
|---	|---	|---	|---	|---	|---	|---	|
|`cluster_id`	|STRING	|-	|Pulse	|UUID of the customer cluster	|`1c821e9c-eebd-4a4a-911e-a0a1b1700000`	|	|
|`cluster_arn`	|STRING	|-	|Pulse	|ARN of the EKS cluster	|`arn:aws:eks:us-west-2:123456789012:cluster/my-cluster`	|	|
|`uid`	|STRING	|	|	|Unique ID for this node (Kubernetes UID)	|`ff701062-58d0-4f53-8ff5-32285c636f59`	|	|
|`name`	|STRING	|	|	|Name of the node	|`ip-10-0-1-100.us-west-2.compute.internal`	|	|
|`cluster_name`	|STRING	|-	|Pulse	|Name of the cluster	|`my-eks-cluster`	|	|
|`event_type`	|STRING	|-	|Pulse	|K8s resource lifecycle event type: ADD (created), UPDATE (mutated), DELETE (deleted)	|`ADD`, `UPDATE`, `DELETE`	|	|
|`event_timestamp`	|STRING	|-	|Pulse	|Time the event was processed at Pulse	|`2024-01-30T18:30:00Z`	|	|
|`region`	|STRING	|-	|Pulse	|AWS region where the node is located	|`us-west-2`	|	|
|`account_id`	|STRING	|-	|Pulse	|AWS account ID	|123456789012	|	|
|`apiVersion`	|STRING	|	|	|Version of the Kubernetes API used	|`v1`	|[K8s API](https://kubernetes.io/docs/reference/using-api/#api-versioning)	|
|`kind`	|STRING	|	|	|K8s resource type, always "Node" for this table	|`Node`	|	|
|`annotations`	|STRING (JSON)	|	|	|Key-value pairs for storing arbitrary metadata. Used by tools and libraries	|`{"node.alpha.kubernetes.io/ttl":"0"}`	|[Annotations](https://kubernetes.io/docs/concepts/overview/working-with-objects/annotations/)	|
|`finalizers`	|STRING (JSON)	|	|	|List of finalizers that must be cleared before deletion. Blocks deletion until conditions are met	|`["kubernetes.io/pv-protection"]`	|[Finalizers](https://kubernetes.io/docs/concepts/overview/working-with-objects/finalizers/)	|
|`labels`	|STRING (JSON)	|	|	|Key-value pairs for organizing and selecting nodes. Used for scheduling and grouping	|`{"kubernetes.io/hostname":"node1","node-role.kubernetes.io/worker":""}`	|[Labels](https://kubernetes.io/docs/concepts/overview/working-with-objects/labels/)	|
|`owner_references`	|STRING (JSON)	|	|	|List of objects that own this node. Used for garbage collection	|`[{"apiVersion":"v1","kind":"Machine","name":"machine-1","uid":"abc123"}]`	|[Owners](https://kubernetes.io/docs/concepts/overview/working-with-objects/owners-dependents/)	|
|`creationTimestamp`	|STRING	|TIMESTAMP	|	|Timestamp when the node was created	|`2024-01-15T10:00:00Z`	|	|
|`deletionGracePeriodSeconds`	|BIGINT	|	|	|Grace period in seconds before force-deletion after deletion request	|30	|	|
|`deletionTimestamp`	|STRING	|TIMESTAMP	|	|Timestamp when node deletion was requested (null if still active)	|`2024-02-01T12:00:00Z`, `null`	|	|
|`generation`	|BIGINT	|	|	|Counter incremented when spec field is modified. Detects spec vs status-only changes	|5	|[Generation](https://kubernetes.io/docs/tasks/extend-kubernetes/custom-resources/custom-resource-definitions/#status-subresource)	|
|`resourceVersion`	|STRING	|	|	|Server's internal version of this object. Changes with every update (optimistic concurrency control)	|18098774	|[ResourceVersion](https://kubernetes.io/docs/reference/using-api/api-concepts/#resource-versions)	|
|`selfLink`	|STRING	|	|	|Deprecated. URL representing this object	|`/api/v1/nodes/node1`	|	|
|`managedFields`	|STRING (JSON)	|	|	|Tracks which fields are managed by which managers. Used for server-side apply	|`[{"manager":"kubectl","operation":"Update","apiVersion":"v1"}]`	|[Server-Side Apply](https://kubernetes.io/docs/reference/using-api/server-side-apply/)	|
|`externalID`	|STRING	|	|	|External ID assigned by cloud provider. Deprecated, use providerID instead	|`i-1234567890abcdef0`	|	|
|`podCIDR`	|STRING	|	|	|IP range assigned to this node for pod IPs (single CIDR)	|`10.244.1.0/24`	|	|
|`providerID`	|STRING	|	|	|Cloud provider-specific ID for this node	|`aws:///us-west-2a/i-1234567890abcdef0`	|	|
|`unschedulable`	|BOOLEAN	|	|	|If true, new pods cannot be scheduled on this node (cordoned)	|`true`, `false`	|[Cordon](https://kubernetes.io/docs/reference/kubectl/generated/kubectl_cordon/)	|
|`configSource`	|STRING (JSON)	|	|	|Deprecated. Source of node configuration	|`{"configMap":{"name":"node-config","namespace":"kube-system"}}`	|	|
|`podCIDRs`	|STRING (JSON)	|	|	|IP ranges assigned to this node for pod IPs (supports dual-stack)	|`["10.244.1.0/24","fd00:10:244:1::/64"]`	|[Dual-Stack](https://kubernetes.io/docs/concepts/services-networking/dual-stack/)	|
|`taints`	|STRING (JSON)	|	|	|Taints prevent pods from scheduling unless they tolerate the taint. Used for dedicated nodes	|`[{"key":"node-role.kubernetes.io/master","effect":"NoSchedule"}]`	|[Taints](https://kubernetes.io/docs/concepts/scheduling-eviction/taint-and-toleration/)	|
|`phase`	|STRING	|	|	|Current lifecycle phase of the node: Pending, Running, Terminated	|`Running`	|	|
|`addresses`	|STRING (JSON)	|	|	|Network addresses of the node: Hostname, ExternalIP, InternalIP	|`[{"type":"InternalIP","address":"10.0.1.100"},{"type":"Hostname","address":"node1"}]`	|[Addresses](https://kubernetes.io/docs/reference/node/node-status/#addresses)	|
|`allocatable`	|STRING (JSON)	|	|	|Resources available for pods (capacity minus system reservations)	|`{"cpu":"3920m","memory":"15Gi","pods":"110"}`	|[Allocatable](https://kubernetes.io/docs/reference/node/node-status/#capacity-and-allocatable)	|
|`capacity`	|STRING (JSON)	|	|	|Total resources on the node before system reservations	|`{"cpu":"4","memory":"16Gi","pods":"110"}`	|[Capacity](https://kubernetes.io/docs/reference/node/node-status/#capacity-and-allocatable)	|
|`conditions`	|STRING (JSON)	|	|	|Current status conditions: Ready, MemoryPressure, DiskPressure, PIDPressure, NetworkUnavailable	|`[{"type":"Ready","status":"True","reason":"KubeletReady","message":"kubelet is ready"}]`	|[Conditions](https://kubernetes.io/docs/reference/node/node-status/#conditions)	|
|`config`	|STRING (JSON)	|	|	|Status of node configuration	|`{"assigned":{"kubelet":{"configMap":{"name":"node-config"}}}}`	|	|
|`daemonEndpoints`	|STRING (JSON)	|	|	|Endpoints of daemons running on the node (kubelet port)	|`{"kubeletEndpoint":{"Port":10250}}`	|	|
|`features`	|STRING (JSON)	|	|	|Declared features enabled on this node via feature gates	|`{"features":["NodeSwap","DynamicResourceAllocation"]}`	|[Features](https://kubernetes.io/docs/reference/node/node-status/#declared-features)	|
|`images`	|STRING (JSON)	|	|	|List of container images present on the node with sizes	|`[{"names":["nginx:1.21"],"sizeBytes":142000000}]`	|	|
|`nodeInfo`	|STRING (JSON)	|	|	|System information: OS, kernel version, kubelet version, container runtime	|`{"architecture":"amd64","kernelVersion":"5.10.0","kubeletVersion":"v1.28.0","osImage":"Amazon Linux 2"}`	|[NodeInfo](https://kubernetes.io/docs/reference/node/node-status/#info)	|
|`runtimeHandlers`	|STRING (JSON)	|	|	|Available container runtime handlers on this node	|`[{"name":"runc","features":{"recursiveReadOnlyMounts":true}}]`	|	|
|`snapshot_timestamp`	|TIMESTAMP	|-	|Analytics	|When this data snapshot was captured	|`2024-01-30T18:00:00Z`	|	|

## Pod

### Metadata Source

Pre-prod schema and sample data : https://tiny.amazon.com/um810e1o/IsenLink
DB : `resources_snapshot`
table : `pods_v1`

### Schema Description

|Column Name	|Type	|Assumed Type	|Source	|Description	|Example	|Ref Link	|
|---	|---	|---	|---	|---	|---	|---	|
|`cluster_id`	|STRING	|-	|Pulse	|UUID of the customer cluster	|`1c821e9c-eebd-4a4a-911e-a0a1b1700000`	|	|
|`cluster_arn`	|STRING	|-	|Pulse	|ARN of the EKS cluster	|`arn:aws:eks:us-west-2:123456789012:cluster/my-cluster`	|	|
|`uid`	|STRING	|	|	|Unique ID for this pod (Kubernetes UID)	|`ff701062-58d0-4f53-8ff5-32285c636f59`	|	|
|`name`	|STRING	|	|	|Name of the pod	|`nginx-deployment-7d64c8f5d9-abc12`	|	|
|`namespace`	|STRING	|	|	|Namespace where the pod resides	|`default`, `kube-system`	|[Namespaces](https://kubernetes.io/docs/concepts/overview/working-with-objects/namespaces/)	|
|`cluster_name`	|STRING	|-	|Pulse	|Name of the cluster	|`my-eks-cluster`	|	|
|`event_type`	|STRING	|-	|Pulse	|K8s resource lifecycle event type: ADD (created), UPDATE (mutated), DELETE (deleted)	|`ADD`, `UPDATE`, `DELETE`	|	|
|`event_timestamp`	|STRING	|-	|Pulse	|Time the event was processed at Pulse	|`2024-01-30T18:30:00Z`	|	|
|`region`	|STRING	|-	|Pulse	|AWS region where the pod is running	|`us-west-2`	|	|
|`account_id`	|STRING	|-	|Pulse	|AWS account ID	|123456789012	|	|
|`apiVersion`	|STRING	|	|	|Version of the Kubernetes API used	|`v1`	|	|
|`kind`	|STRING	|	|	|K8s resource type, always "Pod" for this table	|`Pod`	|	|
|`generateName`	|STRING	|	|	|Prefix for generating pod name. Used by controllers (Deployment, StatefulSet)	|`nginx-deployment-7d64c8f5d9-`	|	|
|`annotations`	|STRING (JSON)	|	|	|Key-value pairs for storing arbitrary metadata	|`{"kubectl.kubernetes.io/last-applied-configuration":"..."}`	|[Annotations](https://kubernetes.io/docs/concepts/overview/working-with-objects/annotations/)	|
|`finalizers`	|STRING (JSON)	|	|	|List of finalizers that must be cleared before deletion	|`["kubernetes.io/pvc-protection"]`	|[Finalizers](https://kubernetes.io/docs/concepts/overview/working-with-objects/finalizers/)	|
|`labels`	|STRING (JSON)	|	|	|Key-value pairs for organizing and selecting pods	|`{"app":"nginx","version":"v1"}`	|[Labels](https://kubernetes.io/docs/concepts/overview/working-with-objects/labels/)	|
|`owner_references`	|STRING (JSON)	|	|	|List of objects that own this pod (for garbage collection)	|`[{"apiVersion":"apps/v1","kind":"ReplicaSet","name":"nginx-7d64c8f5d9"}]`	|[Owners](https://kubernetes.io/docs/concepts/overview/working-with-objects/owners-dependents/)	|
|`activeDeadlineSeconds`	|BIGINT	|	|	|Max time pod can run before system terminates it. Used for Jobs	|3600	|	|
|`automountServiceAccountToken`	|BOOLEAN	|	|	|Whether to auto-mount service account token into containers	|`true`, `false`	|[ServiceAccount](https://kubernetes.io/docs/concepts/security/service-accounts/)	|
|`dnsPolicy`	|STRING	|	|	|DNS resolution policy: ClusterFirst, Default, ClusterFirstWithHostNet, None	|`ClusterFirst`	|[DNS Policy](https://kubernetes.io/docs/concepts/services-networking/dns-pod-service/)	|
|`enableServiceLinks`	|BOOLEAN	|	|	|Whether to inject service environment variables into containers	|`true`, `false`	|	|
|`hostIPC`	|BOOLEAN	|	|	|Use host's IPC namespace. Security risk if true	|FALSE	|[Host Namespaces](https://kubernetes.io/docs/concepts/security/pod-security-standards/)	|
|`hostNetwork`	|BOOLEAN	|	|	|Use host's network namespace. Pod gets host's IP	|FALSE	|	|
|`hostPID`	|BOOLEAN	|	|	|Use host's PID namespace. Can see host processes	|FALSE	|	|
|`hostUsers`	|BOOLEAN	|	|	|Use host's user namespace. Affects UID/GID mapping	|TRUE	|[User Namespaces](https://kubernetes.io/docs/concepts/workloads/pods/user-namespaces/)	|
|`hostname`	|STRING	|	|	|Hostname of the pod. Defaults to pod name if not set	|`my-pod`	|	|
|`nodeName`	|STRING	|	|	|Name of the node where pod is scheduled/running	|`ip-10-0-1-100.us-west-2.compute.internal`	|	|
|`preemptionPolicy`	|STRING	|	|	|Whether this pod can preempt lower-priority pods: PreemptLowerPriority, Never	|`PreemptLowerPriority`	|[Preemption](https://kubernetes.io/docs/concepts/scheduling-eviction/pod-priority-preemption/)	|
|`priority`	|BIGINT	|	|	|Priority value. Higher values = higher priority. Used for scheduling and preemption	|1000	|[Priority](https://kubernetes.io/docs/concepts/scheduling-eviction/pod-priority-preemption/)	|
|`priorityClassName`	|STRING	|	|	|Name of PriorityClass. Determines priority value	|`system-cluster-critical`	|	|
|`restartPolicy`	|STRING	|	|	|When to restart containers: Always, OnFailure, Never	|`Always`	|[Restart Policy](https://kubernetes.io/docs/concepts/workloads/pods/pod-lifecycle/#restart-policy)	|
|`runtimeClassName`	|STRING	|	|	|Runtime handler to use (e.g., different container runtimes)	|`gvisor`, `kata`	|[RuntimeClass](https://kubernetes.io/docs/concepts/containers/runtime-class/)	|
|`schedulerName`	|STRING	|	|	|Name of scheduler to use for this pod	|`default-scheduler`	|	|
|`serviceAccount`	|STRING	|	|	|Deprecated. Use serviceAccountName instead	|`default`	|	|
|`serviceAccountName`	|STRING	|	|	|Name of ServiceAccount to use for this pod	|`my-service-account`	|[ServiceAccount](https://kubernetes.io/docs/concepts/security/service-accounts/)	|
|`setHostnameAsFQDN`	|BOOLEAN	|	|	|Set pod's hostname to Fully Qualified Domain Name	|FALSE	|	|
|`shareProcessNamespace`	|BOOLEAN	|	|	|Share single process namespace between all containers in pod	|FALSE	|[Process Namespace](https://kubernetes.io/docs/tasks/configure-pod-container/share-process-namespace/)	|
|`subdomain`	|STRING	|	|	|Subdomain for pod's hostname. Used with headless services	|`my-subdomain`	|	|
|`terminationGracePeriodSeconds`	|BIGINT	|	|	|Grace period before force-killing pod after deletion request	|30	|	|
|`affinity`	|STRING (JSON)	|	|	|Node/pod affinity and anti-affinity rules for scheduling	|`{"nodeAffinity":{"requiredDuringSchedulingIgnoredDuringExecution":...}}`	|[Affinity](https://kubernetes.io/docs/concepts/scheduling-eviction/assign-pod-node/#affinity-and-anti-affinity)	|
|`dnsConfig`	|STRING (JSON)	|	|	|Custom DNS configuration (nameservers, searches, options)	|`{"nameservers":["1.1.1.1"],"searches":["my.dns.search.suffix"]}`	|[DNS Config](https://kubernetes.io/docs/concepts/services-networking/dns-pod-service/#pod-dns-config)	|
|`hostAliases`	|STRING (JSON)	|	|	|Additional entries for pod's /etc/hosts file	|`[{"ip":"127.0.0.1","hostnames":["foo.local"]}]`	|[Host Aliases](https://kubernetes.io/docs/tasks/network/customize-hosts-file-for-pods/)	|
|`imagePullSecrets`	|STRING (JSON)	|	|	|Secrets for pulling images from private registries	|`[{"name":"regcred"}]`	|[Image Pull Secrets](https://kubernetes.io/docs/concepts/containers/images/#specifying-imagepullsecrets-on-a-pod)	|
|`nodeSelector`	|STRING (JSON)	|	|	|Key-value pairs for selecting nodes (simple node selection)	|`{"disktype":"ssd","kubernetes.io/arch":"amd64"}`	|[NodeSelector](https://kubernetes.io/docs/concepts/scheduling-eviction/assign-pod-node/#nodeselector)	|
|`os`	|STRING (JSON)	|	|	|OS requirements for the pod (linux or windows)	|`{"name":"linux"}`	|	|
|`overhead`	|STRING (JSON)	|	|	|Resource overhead for running the pod (added to container requests)	|`{"cpu":"100m","memory":"50Mi"}`	|[Overhead](https://kubernetes.io/docs/concepts/scheduling-eviction/pod-overhead/)	|
|`readinessGates`	|STRING (JSON)	|	|	|Additional conditions that must be true before pod is ready	|`[{"conditionType":"www.example.com/feature-1"}]`	|[Readiness Gates](https://kubernetes.io/docs/concepts/workloads/pods/pod-lifecycle/#pod-readiness-gate)	|
|`resourceClaims`	|STRING (JSON)	|	|	|Dynamic resource allocation claims (DRA)	|`[{"name":"gpu-claim"}]`	|[DRA](https://kubernetes.io/docs/concepts/scheduling-eviction/dynamic-resource-allocation/)	|
|`resources`	|STRING (JSON)	|	|	|Aggregate resource requirements for all containers	|`{"limits":{"cpu":"2","memory":"4Gi"},"requests":{"cpu":"1","memory":"2Gi"}}`	|	|
|`schedulingGates`	|STRING (JSON)	|	|	|Gates that prevent pod from being scheduled until cleared	|`[{"name":"example.com/my-gate"}]`	|[Scheduling Gates](https://kubernetes.io/docs/concepts/scheduling-eviction/pod-scheduling-readiness/)	|
|`securityContext`	|STRING (JSON)	|	|	|Pod-level security settings (fsGroup, runAsUser, seLinux, etc.)	|`{"runAsNonRoot":true,"fsGroup":2000}`	|[Security Context](https://kubernetes.io/docs/tasks/configure-pod-container/security-context/)	|
|`tolerations`	|STRING (JSON)	|	|	|Tolerations allow pod to schedule on nodes with matching taints	|`[{"key":"node-role.kubernetes.io/master","effect":"NoSchedule"}]`	|[Tolerations](https://kubernetes.io/docs/concepts/scheduling-eviction/taint-and-toleration/)	|
|`topologySpreadConstraints`	|STRING (JSON)	|	|	|Rules for spreading pods across topology domains (zones, nodes)	|`[{"maxSkew":1,"topologyKey":"topology.kubernetes.io/zone"}]`	|[Topology Spread](https://kubernetes.io/docs/concepts/scheduling-eviction/topology-spread-constraints/)	|
|`volumes`	|STRING (JSON)	|	|	|List of volumes that can be mounted by containers	|`[{"name":"config","configMap":{"name":"app-config"}}]`	|[Volumes](https://kubernetes.io/docs/concepts/storage/volumes/)	|
|`creationTimestamp`	|STRING	|	|	|Timestamp when the pod was created	|`2024-01-15T10:00:00Z`	|	|
|`deletionGracePeriodSeconds`	|BIGINT	|	|	|Grace period in seconds before force-deletion	|30	|	|
|`deletionTimestamp`	|STRING	|	|	|Timestamp when pod deletion was requested (null if still active)	|`2024-02-01T12:00:00Z`, `null`	|	|
|`generation`	|BIGINT	|	|	|Counter incremented when spec field is modified	|5	|	|
|`resourceVersion`	|STRING	|BIGINT	|	|Server's internal version of this object. Changes with every update	|18098774	|	|
|`selfLink`	|STRING	|	|	|Deprecated. URL representing this object	|`/api/v1/namespaces/default/pods/nginx`	|	|
|`managedFields`	|STRING (JSON)	|	|	|Tracks which fields are managed by which managers	|`[{"manager":"kubectl","operation":"Update"}]`	|[Server-Side Apply](https://kubernetes.io/docs/reference/using-api/server-side-apply/)	|
|`hostIP`	|STRING	|	|	|IP address of the host where pod is running	|`10.0.1.100`	|	|
|`message`	|STRING	|	|	|Human-readable message about pod status (usually for failures)	|`Pod failed to start: ImagePullBackOff`	|	|
|`nominatedNodeName`	|STRING	|	|	|Node nominated by scheduler for this pod (preemption scenario)	|`ip-10-0-1-200.us-west-2.compute.internal`	|	|
|`observedGeneration`	|BIGINT	|	|	|Most recent generation observed by the controller	|5	|	|
|`phase`	|STRING	|	|	|Current lifecycle phase: Pending, Running, Succeeded, Failed, Unknown	|`Running`	|[Pod Phase](https://kubernetes.io/docs/concepts/workloads/pods/pod-lifecycle/#pod-phase)	|
|`podIP`	|STRING	|	|	|IP address allocated to the pod (primary IP)	|`10.244.1.5`	|	|
|`qosClass`	|STRING	|	|	|Quality of Service class: Guaranteed, Burstable, BestEffort	|`Burstable`	|[QoS Classes](https://kubernetes.io/docs/concepts/workloads/pods/pod-qos/)	|
|`reason`	|STRING	|	|	|Brief reason for pod's current phase (usually for failures)	|`Evicted`, `OutOfMemory`	|	|
|`resize`	|STRING	|	|	|Status of in-place resource resize: Proposed, InProgress, Deferred, Infeasible	|`InProgress`	|[Resize](https://kubernetes.io/docs/tasks/configure-pod-container/resize-container-resources/)	|
|`startTime`	|STRING	|TIMESTAMP	|	|Timestamp when pod was acknowledged by kubelet	|`2024-01-15T10:00:05Z`	|	|
|`conditions`	|STRING (JSON)	|	|	|Current conditions: PodScheduled, Initialized, ContainersReady, Ready	|`[{"type":"Ready","status":"True","lastTransitionTime":"2024-01-15T10:00:10Z"}]`	|[Conditions](https://kubernetes.io/docs/concepts/workloads/pods/pod-lifecycle/#pod-conditions)	|
|`hostIPs`	|STRING (JSON)	|	|	|IP addresses of the host (supports dual-stack)	|`[{"ip":"10.0.1.100"},{"ip":"fd00::1"}]`	|	|
|`podIPs`	|STRING (JSON)	|	|	|IP addresses allocated to the pod (supports dual-stack)	|`[{"ip":"10.244.1.5"},{"ip":"fd00:10:244:1::5"}]`	|	|
|`resourceClaimStatuses`	|STRING (JSON)	|	|	|Status of dynamic resource allocation claims	|`[{"name":"gpu-claim","resourceClaimName":"gpu-claim-abc"}]`	|	|
|`snapshot_timestamp`	|TIMESTAMP	|-	|Analytics	|When this data snapshot was captured	|`2024-01-30T18:00:00Z`	|	|

## Container

### Metadata Source

Pre-prod schema and sample data : https://tiny.amazon.com/um810e1o/IsenLink
DB : `resources_snapshot`
table : `containers_v1`

### Schema Description

|Column Name	|Type	|Assumed Type	|Source	|Description	|Example	|Ref Link	|
|---	|---	|---	|---	|---	|---	|---	|
|`cluster_id`	|STRING	|-	|Pulse	|UUID of the customer cluster	|`1c821e9c-eebd-4a4a-911e-a0a1b1700000`	|	|
|`pod_uid`	|STRING	|	|	|Unique ID of the pod that owns this container	|`ff701062-58d0-4f53-8ff5-32285c636f59`	|	|
|`pod_name`	|STRING	|	|	|Name of the pod	|`nginx-deployment-7d64c8f5d9-abc12`	|	|
|`pod_namespace`	|STRING	|	|	|Namespace where the pod resides	|`default`, `kube-system`	|	|
|`cluster_name`	|STRING	|-	|Pulse	|Name of the cluster	|`my-eks-cluster`	|	|
|`account_id`	|STRING	|-	|Pulse	|AWS account ID	|123456789012	|	|
|`resource_version`	|STRING	|	|	|Resource version of the pod (not container-specific)	|18098774	|	|
|`event_type`	|STRING	|-	|Pulse	|K8s resource lifecycle event type: ADD, UPDATE, DELETE	|`ADD`, `UPDATE`, `DELETE`	|	|
|`event_timestamp`	|STRING	|-	|Pulse	|Time the event was processed at Pulse	|`2024-01-30T18:30:00Z`	|	|
|`deletion_timestamp`	|STRING	|	|	|Timestamp when pod deletion was requested	|`2024-02-01T12:00:00Z`, `null`	|	|
|`container_name`	|STRING	|	|	|Name of the container within the pod	|`nginx`, `sidecar-proxy`	|	|
|`container_type`	|STRING	|	|	|Type of container: "container" (regular) or "initContainer"	|`container`, `initContainer`	|[Init Containers](https://kubernetes.io/docs/concepts/workloads/pods/init-containers/)	|
|`image`	|STRING	|	|	|Container image name and tag	|`nginx:1.21`, `gcr.io/my-project/app:v1.0.0`	|[Images](https://kubernetes.io/docs/concepts/containers/images/)	|
|`imagePullPolicy`	|STRING	|	|	|When to pull the image: Always, Never, IfNotPresent	|`IfNotPresent`, `Always`	|[Image Pull Policy](https://kubernetes.io/docs/concepts/containers/images/#image-pull-policy)	|
|`name`	|STRING	|	|	|Name of the container (same as container_name)	|`nginx`	|	|
|`stdin`	|BOOLEAN	|	|	|Whether to allocate stdin for this container	|FALSE	|	|
|`stdinOnce`	|BOOLEAN	|	|	|Whether stdin is closed after first attach	|FALSE	|	|
|`terminationMessagePath`	|STRING	|	|	|Path where container writes termination message	|`/dev/termination-log`	|	|
|`terminationMessagePolicy`	|STRING	|	|	|How to populate termination message: File, FallbackToLogsOnError	|`File`	|	|
|`tty`	|BOOLEAN	|	|	|Whether to allocate a TTY for this container	|FALSE	|	|
|`workingDir`	|STRING	|	|	|Working directory for container's entrypoint	|`/app`	|	|
|`args`	|STRING (JSON)	|	|	|Arguments to the entrypoint. Overrides CMD in Dockerfile	|`["--port=8080","--config=/etc/app/config.yaml"]`	|	|
|`command`	|STRING (JSON)	|	|	|Entrypoint array. Overrides ENTRYPOINT in Dockerfile	|`["/bin/sh","-c","nginx -g 'daemon off;'"]`	|	|
|`env`	|STRING (JSON)	|	|	|Environment variables for the container	|`[{"name":"DB_HOST","value":"mysql.default.svc"},{"name":"API_KEY","valueFrom":{"secretKeyRef":{"name":"api-secret","key":"key"}}}]`	|[Env Variables](https://kubernetes.io/docs/tasks/inject-data-application/define-environment-variable-container/)	|
|`envFrom`	|STRING (JSON)	|	|	|Sources to populate environment variables (ConfigMap, Secret)	|`[{"configMapRef":{"name":"app-config"}},{"secretRef":{"name":"app-secrets"}}]`	|	|
|`lifecycle`	|STRING (JSON)	|	|	|Lifecycle hooks: postStart and preStop handlers	|`{"preStop":{"exec":{"command":["/bin/sh","-c","nginx -s quit"]}}}`	|[Lifecycle Hooks](https://kubernetes.io/docs/concepts/containers/container-lifecycle-hooks/)	|
|`livenessProbe`	|STRING (JSON)	|	|	|Probe to check if container is alive. Restarts if fails	|`{"httpGet":{"path":"/healthz","port":8080},"initialDelaySeconds":15,"periodSeconds":10}`	|[Liveness Probe](https://kubernetes.io/docs/tasks/configure-pod-container/configure-liveness-readiness-startup-probes/)	|
|`ports`	|STRING (JSON)	|	|	|List of ports to expose from the container	|`[{"name":"http","containerPort":80,"protocol":"TCP"}]`	|	|
|`readinessProbe`	|STRING (JSON)	|	|	|Probe to check if container is ready to serve traffic. Removes from service if fails	|`{"httpGet":{"path":"/ready","port":8080},"initialDelaySeconds":5,"periodSeconds":5}`	|[Readiness Probe](https://kubernetes.io/docs/tasks/configure-pod-container/configure-liveness-readiness-startup-probes/)	|
|`resizePolicy`	|STRING (JSON)	|	|	|Policy for in-place resource resize: RestartContainer or NotRequired	|`[{"resourceName":"cpu","restartPolicy":"NotRequired"}]`	|[Resize Policy](https://kubernetes.io/docs/tasks/configure-pod-container/resize-container-resources/)	|
|`resources`	|STRING (JSON)	|	|	|Resource requests and limits (CPU, memory, ephemeral-storage)	|`{"requests":{"cpu":"100m","memory":"128Mi"},"limits":{"cpu":"500m","memory":"512Mi"}}`	|[Resources](https://kubernetes.io/docs/concepts/configuration/manage-resources-containers/)	|
|`restartPolicy`	|STRING	|	|	|Restart policy for this container (sidecar containers only): Always	|`Always`	|[Sidecar Containers](https://kubernetes.io/docs/concepts/workloads/pods/sidecar-containers/)	|
|`securityContext`	|STRING (JSON)	|	|	|Container-level security settings: runAsUser, capabilities, readOnlyRootFilesystem, etc.	|`{"runAsNonRoot":true,"readOnlyRootFilesystem":true,"allowPrivilegeEscalation":false}`	|[Security Context](https://kubernetes.io/docs/tasks/configure-pod-container/security-context/)	|
|`startupProbe`	|STRING (JSON)	|	|	|Probe to check if container has started. Disables liveness/readiness until succeeds	|`{"httpGet":{"path":"/startup","port":8080},"failureThreshold":30,"periodSeconds":10}`	|[Startup Probe](https://kubernetes.io/docs/tasks/configure-pod-container/configure-liveness-readiness-startup-probes/)	|
|`volumeDevices`	|STRING (JSON)	|	|	|Block devices to be used by the container	|`[{"name":"data","devicePath":"/dev/xvda"}]`	|[Volume Devices](https://kubernetes.io/docs/concepts/storage/persistent-volumes/#raw-block-volume-support)	|
|`volumeMounts`	|STRING (JSON)	|	|	|Volumes to mount into the container's filesystem	|`[{"name":"config","mountPath":"/etc/config","readOnly":true}]`	|[Volume Mounts](https://kubernetes.io/docs/concepts/storage/volumes/)	|
|`targetContainerName`	|STRING	|	|	|For ephemeral containers: name of container to target for debugging	|`nginx`	|[Ephemeral Containers](https://kubernetes.io/docs/concepts/workloads/pods/ephemeral-containers/)	|
|`containerID`	|STRING	|	|	|Runtime-specific container ID. Format: <runtime>://<container-id>	|`containerd://abc123def456`, `docker://789ghi012jkl`	|	|
|`imageID`	|STRING	|	|	|Full image ID including digest	|`docker.io/library/nginx@sha256:abc123...`	|	|
|`ready`	|BOOLEAN	|	|	|Whether container passed readiness probe and is ready to serve traffic	|`true`, `false`	|	|
|`restartCount`	|BIGINT	|	|	|Number of times the container has been restarted	|`0`, `5`	|	|
|`started`	|BOOLEAN	|	|	|Whether container has started (startup probe passed or no startup probe)	|`true`, `false`	|	|
|`allocatedResources`	|STRING (JSON)	|	|	|Resources allocated to the container by the node	|`{"cpu":"100m","memory":"128Mi"}`	|	|
|`allocatedResourcesStatus`	|STRING (JSON)	|	|	|Status of allocated resources during resize	|`[{"name":"cpu","status":"Applied"}]`	|	|
|`lastState`	|STRING (JSON)	|	|	|Details about container's last termination (if restarted)	|`{"terminated":{"exitCode":1,"reason":"Error","startedAt":"2024-01-30T10:00:00Z","finishedAt":"2024-01-30T10:05:00Z"}}`	|	|
|`state`	|STRING (JSON)	|	|	|Current state of the container: waiting, running, or terminated	|`{"running":{"startedAt":"2024-01-30T10:05:10Z"}}`	|[Container States](https://kubernetes.io/docs/concepts/workloads/pods/pod-lifecycle/#container-states)	|
|`stopSignal`	|STRING	|	|	|Signal to stop the container (SIGTERM, SIGKILL, etc.)	|`SIGTERM`	|	|
|`user`	|STRING (JSON)	|	|	|User and group information for the running container	|`{"linux":{"uid":1000,"gid":3000}}`	|	|
|`volumeMountStatuses`	|STRING (JSON)	|	|	|Status of volume mounts (from container status, not spec)	|`[{"name":"config","mountPath":"/etc/config","readOnly":true}]`	|	|
|`snapshot_timestamp`	|TIMESTAMP	|-	|Analytics	|When this data snapshot was captured	|`2024-01-30T18:00:00Z`	|	|

## Node Pool

### Metadata Source

Pre-prod schema and sample data : https://tiny.amazon.com/um810e1o/IsenLink
DB : `resources_snapshot`
table : `nodepools_v1`

### Schema Description

|Column Name	|Type	|Assumed Type	|Source	|Description	|Example	|Ref Link	|
|---	|---	|---	|---	|---	|---	|---	|
|`cluster_id`	|STRING	|-	|Pulse	|UUID of a customer cluster	|`1c821e9c-eebd-4a4a-911e-a0a1b1700000`	|	|
|`region`	|STRING	|-	|Pulse	|AWS region where the cluster was created	|`us-west-2`	|	|
|`uid`	|STRING	|	|	|Unique ID for this node pool (Kubernetes UID)	|`ff701062-58d0-4f53-8ff5-32285c636f59`	|	|
|`kind`	|STRING	|	|	|K8s resource type, always "NodePool" for this table	|`NodePool`	|	|
|`event_type`	|STRING	|-	|Pulse	|K8s resource lifecycle event type: ADD (created), UPDATE (mutated), DELETE (deleted)	|`ADD`, `UPDATE`, `DELETE`	|	|
|`event_timestamp`	|STRING	|-	|Pulse	|Time the event was processed at Pulse (EKS event processing system)	|`2024-01-30T18:30:00Z`	|	|
|`api_version`	|STRING	|	|	|Version of the Kubernetes API used to create this object	|`karpenter.sh/v1`	|[K8s API](https://kubernetes.io/docs/reference/using-api/#api-versioning)	|
|`creation_timestamp`	|STRING	|	|	|Timestamp when the resource was created	|`2024-01-15T10:00:00Z`	|	|
|`deletion_timestamp`	|STRING	|	|	|Timestamp when resource deletion was requested (null if still active)	|`2024-02-01T12:00:00Z`, `null`	|	|
|`resource_version`	|STRING	|	|	|Server's internal version of this object. Changes with every update (optimistic concurrency control)	|18098774	|[K8s ResourceVersion](https://kubernetes.io/docs/reference/using-api/api-concepts/#resource-versions)	|
|`generation`	|BIGINT	|	|	|Counter incremented when spec field is modified. Allows detecting spec vs status-only changes	|5	|[K8s Generation](https://kubernetes.io/docs/tasks/extend-kubernetes/custom-resources/custom-resource-definitions/#status-subresource)	|
|`replicas`	|BIGINT	|	|	|Optional. When set, maintains fixed node count (static mode). When null, auto-scales based on demand (dynamic mode)	|`5`, `null`	|[Karpenter Static](https://karpenter.sh/docs/concepts/nodepools/#specreplicas)	|
|`template_spec_node_class_ref_group`	|STRING	|	|	|API group of the node class reference (defines infrastructure like EC2 config)	|`karpenter.k8s.aws`	|[NodeClass](https://karpenter.sh/docs/concepts/nodeclasses/)	|
|`template_spec_node_class_ref_kind`	|STRING	|	|	|Kind of the node class reference	|`EC2NodeClass`	|	|
|`template_spec_expire_after`	|STRING	|	|	|Duration after which nodes auto-expire and are replaced. Reduces security risks. Can be "Never"	|`720h`, `Never`	|[Expiration](https://karpenter.sh/docs/concepts/nodepools/#spectemplatespecexpireafter)	|
|`template_spec_termination_grace_period`	|STRING	|	|	|Max time a node can drain before force-deletion. Pods may be force-deleted if their grace period is longer	|`48h`, `30s`	|[Termination](https://karpenter.sh/docs/concepts/nodepools/#spectemplatespecterminationgraceperiod)	|
|`template_spec_requirements`	|STRING (JSON)	|	|	|Node selection constraints: instance types, zones, architectures, capacity types, etc.	|`[{"key":"karpenter.sh/capacity-type","operator":"In","values":["spot"]}]`	|[Requirements](https://karpenter.sh/docs/concepts/nodepools/#spectemplatespecrequirements)	|
|`disruption_consolidation_policy`	|STRING	|	|	|When to consolidate nodes: WhenEmptyOrUnderutilized, WhenEmpty, or Never	|`WhenEmptyOrUnderutilized`, `WhenEmpty`	|[Disruption](https://karpenter.sh/docs/concepts/disruption/)	|
|`disruption_consolidate_after`	|STRING	|	|	|Wait time before consolidating after pod changes. Prevents thrashing. Can be "Never"	|`1m`, `15m`, `Never`	|[Consolidation](https://karpenter.sh/docs/concepts/nodepools/#specdisruption)	|
|`disruption_budgets`	|STRING (JSON)	|	|	|Limits on simultaneous node disruptions. Can specify by percentage or count, with schedules	|`[{"nodes":"10%"},{"nodes":"0","schedule":"0 9 * * mon-fri","duration":"8h"}]`	|[Budgets](https://karpenter.sh/docs/concepts/nodepools/#specdisruption)	|
|`limits_cpu`	|STRING	|	|	|Maximum total CPU allowed across all nodes. Prevents over-provisioning	|`1000`, `500`	|[Limits](https://karpenter.sh/docs/concepts/nodepools/#speclimits)	|
|`limits_memory`	|STRING	|	|	|Maximum total memory allowed across all nodes	|`1000Gi`, `4000Gi`	|	|
|`limits_nodes`	|STRING	|	|	|Maximum number of nodes allowed. Only supported for static NodePools	|`100`, `2k`	|	|
|`weight`	|BIGINT	|	|	|Priority score for NodePool selection. Higher = higher priority. Cannot be set with replicas	|`10`, `50`	|[Weight](https://karpenter.sh/docs/concepts/nodepools/#specweight)	|
|`conditions`	|STRING (JSON)	|	|	|Health/readiness status: type, status, reason, message. Examples: NodeClassReady, Ready	|`[{"type":"Ready","status":"True","reason":"NodePoolReady"}]`	|[Conditions](https://karpenter.sh/docs/concepts/nodepools/#statusconditions)	|
|`node_class_observed_generation`	|BIGINT	|	|	|Last observed generation of the referenced node class. Detects node class changes	|3	|	|
|`nodes`	|BIGINT	|	|	|Current number of nodes actually running (may differ from replicas in static mode)	|`3`, `5`	|[Status](https://karpenter.sh/docs/concepts/nodepools/#statusnodes)	|
|`resources_cpu`	|STRING	|	|	|Total CPU capacity currently available across all nodes	|`48`, `20`	|[Resources](https://karpenter.sh/docs/concepts/nodepools/#statusresources)	|
|`resources_memory`	|STRING	|	|	|Total memory capacity currently available across all nodes	|`192Gi`, `8192Mi`	|	|
|`resources_ephemeral_storage`	|STRING	|	|	|Total temporary disk space currently available across all nodes	|`300Gi`, `100Gi`	|	|
|`snapshot_timestamp`	|TIMESTAMP	|-	|Analytics	|When this data snapshot was captured from the source system	|`2024-01-30T18:00:00Z`	|	|



 
