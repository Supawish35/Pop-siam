const IP = 'localhost'; // Change to your server IP if needed
const PORT = '8765'; // WebSocket server port 

const GOAL = 1500; // Click goal to change the image

class WebSocketClickCounter {
  constructor() {
    this.clickCount = 0;
    this.socket = null;
    this.isConnected = false;
    this.reconnectAttempts = 0;
    this.maxReconnectAttempts = 5;

    this.initializeElements();
    this.setupEventListeners();
    this.connectWebSocket();
  }

  initializeElements() {
    this.clickCounterEl = document.getElementById('clickCounter');
    this.dynamicImageEl = document.getElementById('dynamicImage');
    this.statusEl = document.getElementById('wsStatus');

    this.audio = document.getElementById("myAudio");

    this.originalImageSrc = 'photos/pop2.jpg';
    this.clickedImageSrc = 'photos/pop1.jpg';
    this.goalOriginalImageSrc = 'photos/pop4.png';
    this.goalClickedImageSrc = 'photos/pop3.png';
  }

  setupEventListeners() {
    document.body.addEventListener('mousedown', (e) => {
      this.handleMouseDown(e);
    });

    document.body.addEventListener('mouseup', (e) => {
      this.handleMouseUp(e);
    });
  }

  connectWebSocket() {
    const wsUrl = `ws://${IP}:${PORT}`;
    this.socket = new WebSocket(wsUrl);

    this.socket.onopen = () => {
      this.isConnected = true;
      this.updateStatus('Connected');
      this.startHeartbeat();
      console.log('Connected to WebSocket server');
    };

    this.socket.onmessage = (event) => {
      this.handleServerMessage(JSON.parse(event.data));
    };

    this.socket.onclose = () => {
      this.isConnected = false;
      this.updateStatus('Disconnected');
      console.log('Disconnected from WebSocket server');
      this.attemptReconnect();
    };

    this.socket.onerror = (error) => {
      console.error('WebSocket error:', error);
      this.updateStatus('Error');
    };
  }

  handleServerMessage(data) {
    console.log('Received from server: ', data);

    switch (data.type) {
      case 'init':
        this.clickCount = 0;
        this.updateClickCounter();
        if (data.total_clicks !== undefined) {
          this.updateTotalCounter(data.total_clicks);
        }
        break;

      case 'click_response':
        this.clickCount = data.client_clicks;
        this.updateClickCounter();
        if (data.total_clicks !== undefined) {
          this.updateTotalCounter(data.total_clicks);
        }
        break;

      case 'global_update':
        if (data.total_clicks !== undefined) {
          this.updateTotalCounter(data.total_clicks);
        }
        break;

      case 'pong':
        console.log('Received pong from server');
        break;

      case 'error':
        console.error('Server error:', data.message);
        break;
    }
  }

  startHeartbeat() {
    setInterval(() => {
      if (this.isConnected && this.socket.readyState === WebSocket.OPEN) {
        this.sendToServer({ type: 'ping' });
      }
    }, 30000);
  }

  attemptReconnect() {
    if (this.reconnectAttempts < this.maxReconnectAttempts) {
      this.reconnectAttempts++;
      console.log(`Attempting to reconnect... (${this.reconnectAttempts}/${this.maxReconnectAttempts})`);

      setTimeout(() => {
        this.connectWebSocket();
      }, 2000 * this.reconnectAttempts);
    } else {
      console.log('Max reconnection attempts reached');
      this.updateStatus('Failed to connect');
    }
  }

    //mouse down 
  handleMouseDown(event) {
    if (this.clickCount < GOAL) {
        this.dynamicImageEl.src = this.clickedImageSrc;
    } else {
        this.dynamicImageEl.src = this.goalClickedImageSrc;
    }
    this.audio.play();
    this.createClickEffect(event);
    this.sendClickData(event);
  }

    //mouse up
  handleMouseUp(event) {
    if (this.clickCount < GOAL) {
        this.dynamicImageEl.src = this.originalImageSrc;
    }else {
        this.dynamicImageEl.src = this.goalOriginalImageSrc;
  }
  this.audio.pause();
  this.audio.currentTime = 0; // Reset audio to start
  }

    // Create a click effect at the mouse position
  createClickEffect(event) {
    const effect = document.createElement('div');
    effect.className = 'click-effect';
    effect.style.left = `${event.clientX - 50}px`;
    effect.style.top = `${event.clientY - 50}px`;

    document.body.appendChild(effect);

    setTimeout(() => {
      document.body.removeChild(effect);
    }, 600);
  }

    // Update the click counter display
  updateClickCounter() {
    this.clickCounterEl.textContent = this.clickCount;
    this.clickCounterEl.style.transform = 'scale(1.2)';

    setTimeout(() => {
      this.clickCounterEl.style.transform = 'scale(1)';
    }, 200);
  }

  sendToServer(data) {
    if (this.isConnected && this.socket.readyState === WebSocket.OPEN) {
      this.socket.send(JSON.stringify(data));
      console.log('Sent to server:', data);
    } else {
      console.log('WebSocket not connected, cannot send:', data);
    }
  }

  sendClickData(event) {
    const data = {
      type: 'click',
      timestamp: new Date().toISOString(),
      position: { x: event.clientX, y: event.clientY },
    };

    this.sendToServer(data);

    // Flash status to show activity
    this.statusEl.style.backgroundColor = 'rgba(0, 255, 0, 0.4)';
    setTimeout(() => {
      this.statusEl.style.backgroundColor = 'rgba(0, 255, 0, 0.2)';
    }, 200);
  }

  updateStatus(status) {
    this.statusEl.textContent = status;
    this.statusEl.className = `status ${status.toLowerCase()}`;
  }

  updateTotalCounter(totalCount) {
    if (!this.totalCounterEl) {
      this.totalCounterEl = document.createElement('div');
      this.totalCounterEl.id = 'totalCounter';
      this.totalCounterEl.style.position = 'fixed';
      this.totalCounterEl.style.top = '10px';
      this.totalCounterEl.style.left = '10px';
      this.totalCounterEl.style.backgroundColor = 'rgba(0,0,0,0.6)';
      this.totalCounterEl.style.color = 'white';
      this.totalCounterEl.style.padding = '5px 10px';
      this.totalCounterEl.style.borderRadius = '6px';
      this.totalCounterEl.style.fontSize = '16px';
      this.totalCounterEl.style.zIndex = '1000';
      document.body.appendChild(this.totalCounterEl);
    }
    this.totalCounterEl.textContent = `Total: ${totalCount}`;
  }
}

// Initialize the application when the page loads
document.addEventListener('DOMContentLoaded', () => {
  new WebSocketClickCounter();
});

function main() {
  var name
}