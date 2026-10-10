// Synthetic migration-baseline fixture. Credentials are fake test values.
import com.amazonaws.auth.BasicAWSCredentials;

class BasicAWSCredentialsFixture {
  private final Client client = new Client();
  private String token;

  void configure() {
    String id = "AKIAIOSFODNN7EXAMPLE";
    String secret = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYzabcD12345";
    new BasicAWSCredentials(id, secret);
    this.token = "ghp_FAKEjavaLoginToken1234567890ABCDE";
    client.login(this.token);
  }
}

class Client { void login(String token) {} }
