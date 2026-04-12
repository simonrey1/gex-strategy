    - name: thetadata
      image: ${THETA_IMAGE}
      command: ["java"]
      args: ["-jar", "/theta/ThetaTerminalv3.jar", "--creds-file", "/theta/creds.txt"]
      volumeMounts:
        - name: thetadata-jar
          mountPath: /theta
      ports:
        - containerPort: 25503
          hostPort: 25503
