apiVersion: v1
kind: Pod
metadata:
  name: gex-dev
  labels:
    app: gex-dev
spec:
  containers:
# @include gateway-container.yaml.tpl
# @include theta-container.yaml.tpl
  volumes:
# @include theta-volume.yaml.tpl
