package example;

import org.slf4j.bridge.SLF4JBridgeHandler;

class LogbackLoggingSystem {
    void cleanUp() {
        if (SLF4JBridgeHandler.isInstalled()) {
            SLF4JBridgeHandler.uninstall();
        }
    }
}
