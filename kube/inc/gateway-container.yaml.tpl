    - name: ib-gateway
      image: ${GATEWAY_IMAGE}
      env:
        - name: TWS_USERID
          value: "${TWS_USERID}"
        - name: TWS_PASSWORD
          value: "${TWS_PASSWORD}"
        - name: TRADING_MODE
          value: paper
        - name: READ_ONLY_API
          value: "no"
        - name: TWOFA_TIMEOUT_ACTION
          value: restart
        - name: AUTO_RESTART_TIME
          value: "11:59 PM"
        - name: EXISTING_SESSION_DETECTED_ACTION
          value: primary
        - name: TIME_ZONE
          value: America/New_York
      ports:
        - containerPort: 4004
          hostPort: 4002
        - containerPort: 5900
          hostPort: 5900
