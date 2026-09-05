package tuitest

import (
	"errors"
	"testing"
)

func TestNativeOptionPointersPreserveValidation(t *testing.T) {
	terminal, err := Ephemeral(t.Name(), ClientOptions{Recording: &AutomaticRecording{Mode: RecordingDisabled}})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if closeErr := terminal.Close(); closeErr != nil {
			t.Error(closeErr)
		}
	})
	options := SpawnOptions{Backend: Backend("invalid-native-backend")}
	t.Run("open", func(t *testing.T) {
		_, openErr := terminal.Open(OpenOptions{SpawnOptions: options})
		requireNativeUsageError(t, openErr)
	})
	t.Run("run", func(t *testing.T) {
		_, runErr := terminal.Run("unused-program", nil, options)
		requireNativeUsageError(t, runErr)
	})
}

func requireNativeUsageError(t *testing.T, err error) {
	t.Helper()
	var failure *Error
	if !errors.As(err, &failure) || failure.Kind != UsageError {
		t.Fatalf("expected native usage error, got %v", err)
	}
}
