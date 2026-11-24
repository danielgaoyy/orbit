import { abilityAccessCtrl, common, Permissions } from '@kit.AbilityKit';
import { speechRecognizer } from '@kit.CoreSpeechKit';
import { orbitLogWarn } from './OrbitLogger';

export interface OrbitVoiceCallbacks {
  onPartialResult: (text: string) => void;
  onFinalResult: (text: string) => void;
  onStateChanged: (recording: boolean) => void;
  onError: (message: string) => void;
}

export class OrbitVoiceInputController {
  private engine: speechRecognizer.SpeechRecognitionEngine | null = null;
  private activeSessionId: string = '';
  private isListening: boolean = false;
  private latestTranscript: string = '';

  public isActive(): boolean {
    return this.isListening;
  }

  public async start(
    context: common.Context | undefined,
    callbacks: OrbitVoiceCallbacks
  ): Promise<boolean> {
    if (this.isListening) {
      return true;
    }
    if (!canIUse('SystemCapability.AI.SpeechRecognizer')) {
      callbacks.onError('On-device voice dictation is not supported on this device.');
      return false;
    }
    if (context === undefined) {
      callbacks.onError('Microphone requires an active ability context.');
      return false;
    }

    // Request runtime microphone permission cleanly
    try {
      const atManager = abilityAccessCtrl.createAtManager();
      const perms: Array<Permissions> = ['ohos.permission.MICROPHONE'];
      const grantRes = await atManager.requestPermissionsFromUser(context, perms);
      const granted = grantRes.authResults.length > 0 && grantRes.authResults[0] === 0;
      if (!granted) {
        callbacks.onError('Microphone permission denied.');
        return false;
      }
    } catch (err) {
      orbitLogWarn('OrbitVoice', 'Microphone permission check failed', err as Object);
      callbacks.onError('Unable to request microphone permission.');
      return false;
    }

    try {
      const extraInit: Record<string, Object> = {
        'locate': 'CN',
        'recognizerMode': 'short'
      };
      const engineParams: speechRecognizer.CreateEngineParams = {
        language: 'zh-CN',
        online: 1,
        extraParams: extraInit
      };
      const eng = await speechRecognizer.createEngine(engineParams);
      this.engine = eng;
      this.latestTranscript = '';
      this.activeSessionId = `orbit_voice_${Date.now()}`;

      const listener: speechRecognizer.RecognitionListener = {
        onStart: (_sessionId: string, _eventMessage: string): void => {
          this.isListening = true;
          callbacks.onStateChanged(true);
        },
        onEvent: (_sessionId: string, _eventCode: number, _eventMessage: string): void => {
          // Informational audio level / VAD event
        },
        onResult: (_sessionId: string, result: speechRecognizer.SpeechRecognitionResult): void => {
          const text = result.result !== undefined ? result.result.trim() : '';
          if (text.length > 0) {
            this.latestTranscript = text;
            callbacks.onPartialResult(text);
          }
          if (result.isLast) {
            this.isListening = false;
            callbacks.onStateChanged(false);
            callbacks.onFinalResult(this.latestTranscript);
          }
        },
        onComplete: (_sessionId: string, _eventMessage: string): void => {
          const finalVal = this.latestTranscript;
          this.isListening = false;
          callbacks.onStateChanged(false);
          if (finalVal.length > 0) {
            callbacks.onFinalResult(finalVal);
          }
          this.cleanupEngine();
        },
        onError: (_sessionId: string, _errorCode: number, errorMessage: string): void => {
          this.isListening = false;
          callbacks.onStateChanged(false);
          callbacks.onError(
            errorMessage.length > 0 ? errorMessage : 'Voice recognition interrupted.'
          );
          this.cleanupEngine();
        }
      };

      eng.setListener(listener);

      const audioInfo: speechRecognizer.AudioInfo = {
        audioType: 'pcm',
        sampleRate: 16000,
        soundChannel: 1,
        sampleBit: 16
      };
      const extraStart: Record<string, Object> = {
        'recognitionMode': 0,
        'vadBegin': 2000,
        'vadEnd': 3000,
        'maxAudioDuration': 60000
      };
      const startParams: speechRecognizer.StartParams = {
        sessionId: this.activeSessionId,
        audioInfo: audioInfo,
        extraParams: extraStart
      };
      eng.startListening(startParams);
      this.isListening = true;
      callbacks.onStateChanged(true);
      return true;
    } catch (err) {
      orbitLogWarn('OrbitVoice', 'SpeechRecognizer start failed', err as Object);
      this.isListening = false;
      callbacks.onStateChanged(false);
      this.cleanupEngine();
      callbacks.onError('Voice dictation unavailable on this device.');
      return false;
    }
  }

  public finish(): string {
    const transcript = this.latestTranscript;
    if (this.engine !== null && this.activeSessionId.length > 0) {
      try {
        this.engine.finish(this.activeSessionId);
      } catch (err) {
        orbitLogWarn('OrbitVoice', 'SpeechRecognizer finish warning', err as Object);
      }
    }
    this.isListening = false;
    return transcript;
  }

  public cancel(): void {
    if (this.engine !== null && this.activeSessionId.length > 0) {
      try {
        this.engine.cancel(this.activeSessionId);
      } catch (err) {
        orbitLogWarn('OrbitVoice', 'SpeechRecognizer cancel warning', err as Object);
      }
    }
    this.latestTranscript = '';
    this.isListening = false;
    this.cleanupEngine();
  }

  private cleanupEngine(): void {
    if (this.engine !== null) {
      try {
        this.engine.shutdown();
      } catch (err) {
        orbitLogWarn('OrbitVoice', 'SpeechRecognizer shutdown warning', err as Object);
      }
      this.engine = null;
    }
    this.activeSessionId = '';
  }
}
