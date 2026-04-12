apiVersion: v1
kind: Pod
metadata:
  name: gex-strategy
  labels:
    app: gex-strategy
spec:
  containers:
# @include gateway-container.yaml.tpl
# @include theta-container.yaml.tpl
    - name: strategy
      image: ${STRATEGY_IMAGE}
      env:
        - name: IBKR_HOST
          value: "127.0.0.1"
        - name: IBKR_PORT
          value: "4004"
      command: ${STRATEGY_CMD}
      ports:
        - containerPort: ${DASH_PORT}
          hostPort: ${DASH_PORT}
      livenessProbe:
        httpGet:
          path: /health
          port: ${DASH_PORT}
        initialDelaySeconds: 180
        periodSeconds: 60
        failureThreshold: 5
      readinessProbe:
        httpGet:
          path: /health
          port: ${DASH_PORT}
        initialDelaySeconds: 180
        periodSeconds: 30
        failureThreshold: 5
      volumeMounts:
        - name: data
          mountPath: /app/data
  volumes:
    - name: data
      hostPath:
        path: ${PROJECT_DIR}/data
        type: DirectoryOrCreate
# @include theta-volume.yaml.tpl
